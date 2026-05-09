// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {OwnableUpgradeable} from "@openzeppelin/contracts-upgradeable/access/OwnableUpgradeable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";

import {DataService} from "@graphprotocol/horizon/data-service/DataService.sol";
import {DataServiceFees} from "@graphprotocol/horizon/data-service/extensions/DataServiceFees.sol";
import {
    DataServicePausableUpgradeable
} from "@graphprotocol/horizon/data-service/extensions/DataServicePausableUpgradeable.sol";
import {IGraphPayments} from "@graphprotocol/horizon/interfaces/IGraphPayments.sol";
import {IGraphTallyCollector} from "@graphprotocol/horizon/interfaces/IGraphTallyCollector.sol";

import {IMCPDataService} from "./interfaces/IMCPDataService.sol";

/// @title MCPDataService
/// @notice Decentralised MCP (Model Context Protocol) data service built on The Graph Protocol's
///         Horizon framework. This is the on-chain settlement layer for Compass.
///
/// Indexers provision GRT via HorizonStaking, register here, then add each subgraph
/// deployment they serve as an MCP tool. AI agents pay per tool call via GraphTally
/// (TAP v2) receipts; indexers collect fees by submitting signed RAVs to collect().
///
/// @dev Mirrors RPCDataService structure. Key difference: chain-based registration
///      is replaced by subgraph-deployment-based registration — any provisioned
///      indexer can add any deployment, no governance allowlist required.
///      Deployed as a UUPS upgradeable proxy on Arbitrum One.
contract MCPDataService is
    OwnableUpgradeable,
    UUPSUpgradeable,
    DataService,
    DataServiceFees,
    DataServicePausableUpgradeable,
    IMCPDataService
{
    // -------------------------------------------------------------------------
    // Constants
    // -------------------------------------------------------------------------

    /// @notice Minimum GRT provision to register as a Compass MCP provider.
    uint256 public constant DEFAULT_MIN_PROVISION = 1_000e18; // 1,000 GRT

    /// @notice Fraction of collected fees burned by the data service, in PPM (1% = 10_000).
    uint256 public constant BURN_CUT_PPM = 10_000;

    /// @notice Fraction of collected fees retained by the data service as revenue, in PPM.
    uint256 public constant DATA_SERVICE_CUT_PPM = 10_000;

    /// @notice Absolute lower bound on the thawing period.
    uint64 public constant MIN_THAWING_PERIOD = 14 days;

    /// @notice Stake locked per GRT of fees collected — matches SubgraphService.
    uint256 public constant STAKE_TO_FEES_RATIO = 5;

    // -------------------------------------------------------------------------
    // Storage
    // -------------------------------------------------------------------------

    /// @notice Whether a provider has registered with this service.
    mapping(address => bool) public registeredProviders;

    /// @notice Address that receives collected GRT for each provider.
    mapping(address => address) public paymentsDestination;

    /// @notice Subgraph registrations per provider (active and historical).
    mapping(address => SubgraphEntry[]) internal _providerSubgraphs;

    /// @notice GraphTallyCollector used to redeem TAP receipts on-chain.
    IGraphTallyCollector private immutable GRAPH_TALLY_COLLECTOR;

    /// @notice Governance-adjustable thawing period lower bound.
    uint64 public minThawingPeriod;

    /// @dev Reserved storage slots for future upgrades.
    uint256[50] private _gap;

    // -------------------------------------------------------------------------
    // Constructor
    // -------------------------------------------------------------------------

    /// @param controller The Graph Protocol controller address (GraphDirectory).
    /// @param graphTallyCollector Address of the deployed GraphTallyCollector.
    constructor(address controller, address graphTallyCollector) DataService(controller) {
        GRAPH_TALLY_COLLECTOR = IGraphTallyCollector(graphTallyCollector);
        _disableInitializers();
    }

    // -------------------------------------------------------------------------
    // Initializer
    // -------------------------------------------------------------------------

    /// @notice Initialise the proxy. Called exactly once via ERC1967Proxy deployment.
    /// @param owner_ Initial owner (governance multisig or deployer for testnet).
    /// @param pauseGuardian Address authorised to pause the service in an emergency.
    function initialize(address owner_, address pauseGuardian) external initializer {
        __Ownable_init(owner_);
        __DataService_init();
        __DataServicePausable_init();

        minThawingPeriod = MIN_THAWING_PERIOD;
        _setProvisionTokensRange(DEFAULT_MIN_PROVISION, type(uint256).max);
        _setThawingPeriodRange(MIN_THAWING_PERIOD, type(uint64).max);
        _setVerifierCutRange(0, uint32(1_000_000));
        _setPauseGuardian(pauseGuardian, true);
    }

    // -------------------------------------------------------------------------
    // UUPS
    // -------------------------------------------------------------------------

    function _authorizeUpgrade(address) internal override onlyOwner {}

    // -------------------------------------------------------------------------
    // Governance
    // -------------------------------------------------------------------------

    /// @inheritdoc IMCPDataService
    function setMinThawingPeriod(uint64 period) external onlyOwner {
        if (period < MIN_THAWING_PERIOD) revert ThawingPeriodTooShort(MIN_THAWING_PERIOD, period);
        minThawingPeriod = period;
        emit MinThawingPeriodSet(period);
    }

    // -------------------------------------------------------------------------
    // IDataService — lifecycle
    // -------------------------------------------------------------------------

    /// @notice Register as a Compass MCP provider.
    /// @param serviceProvider The provider's staking address.
    /// @param data ABI-encoded (string endpoint, address paymentsDestination).
    function register(address serviceProvider, bytes calldata data)
        external
        override
        whenNotPaused
        onlyAuthorizedForProvision(serviceProvider)
    {
        if (registeredProviders[serviceProvider]) {
            revert ProviderAlreadyRegistered(serviceProvider);
        }
        _checkProvisionTokens(serviceProvider);
        _checkProvisionParameters(serviceProvider, false);

        (string memory endpoint, address dest) = abi.decode(data, (string, address));
        registeredProviders[serviceProvider] = true;
        paymentsDestination[serviceProvider] = dest == address(0) ? serviceProvider : dest;

        emit ProviderRegistered(serviceProvider, endpoint);
    }

    /// @notice Deregister as a Compass MCP provider.
    /// @dev All active subgraph registrations must be removed first.
    function deregister(address serviceProvider, bytes calldata)
        external
        onlyAuthorizedForProvision(serviceProvider)
    {
        if (!registeredProviders[serviceProvider]) revert ProviderNotRegistered(serviceProvider);
        if (activeSubgraphCount(serviceProvider) > 0) revert ActiveSubgraphsExist(serviceProvider);

        registeredProviders[serviceProvider] = false;
        emit ProviderDeregistered(serviceProvider);
    }

    /// @inheritdoc IMCPDataService
    function setPaymentsDestination(address destination) external {
        if (!registeredProviders[msg.sender]) revert ProviderNotRegistered(msg.sender);
        address dest = destination == address(0) ? msg.sender : destination;
        paymentsDestination[msg.sender] = dest;
        emit PaymentsDestinationSet(msg.sender, dest);
    }

    /// @inheritdoc IMCPDataService
    /// @param data ABI-encoded (bytes32 deploymentId, uint128 pricePerQuery, string endpoint).
    function addSubgraph(address serviceProvider, bytes calldata data)
        external
        override
        whenNotPaused
        onlyAuthorizedForProvision(serviceProvider)
    {
        if (!registeredProviders[serviceProvider]) revert ProviderNotRegistered(serviceProvider);

        (bytes32 deploymentId, uint128 pricePerQuery, string memory endpoint) =
            abi.decode(data, (bytes32, uint128, string));

        // Reactivate an existing (removed) entry rather than pushing a duplicate.
        SubgraphEntry[] storage entries = _providerSubgraphs[serviceProvider];
        for (uint256 i = 0; i < entries.length; i++) {
            if (entries[i].deploymentId == deploymentId) {
                if (entries[i].active) revert SubgraphAlreadyActive(serviceProvider, deploymentId);
                entries[i].active = true;
                entries[i].pricePerQuery = pricePerQuery;
                entries[i].endpoint = endpoint;
                emit SubgraphAdded(serviceProvider, deploymentId, pricePerQuery);
                return;
            }
        }

        entries.push(SubgraphEntry({
            deploymentId: deploymentId,
            pricePerQuery: pricePerQuery,
            endpoint: endpoint,
            active: true
        }));
        emit SubgraphAdded(serviceProvider, deploymentId, pricePerQuery);
    }

    /// @inheritdoc IMCPDataService
    /// @param data ABI-encoded (bytes32 deploymentId).
    function removeSubgraph(address serviceProvider, bytes calldata data)
        external
        override
        onlyAuthorizedForProvision(serviceProvider)
    {
        if (!registeredProviders[serviceProvider]) revert ProviderNotRegistered(serviceProvider);

        bytes32 deploymentId = abi.decode(data, (bytes32));

        SubgraphEntry[] storage entries = _providerSubgraphs[serviceProvider];
        for (uint256 i = 0; i < entries.length; i++) {
            if (entries[i].deploymentId == deploymentId && entries[i].active) {
                entries[i].active = false;
                emit SubgraphRemoved(serviceProvider, deploymentId);
                return;
            }
        }
        revert SubgraphNotFound(serviceProvider, deploymentId);
    }

    /// @notice startService / stopService are aliased to addSubgraph / removeSubgraph
    ///         to satisfy the IDataService interface. Use the named functions directly.
    function startService(address serviceProvider, bytes calldata data)
        external
        override
        whenNotPaused
        onlyAuthorizedForProvision(serviceProvider)
    {
        this.addSubgraph(serviceProvider, data);
    }

    function stopService(address serviceProvider, bytes calldata data)
        external
        override
        onlyAuthorizedForProvision(serviceProvider)
    {
        this.removeSubgraph(serviceProvider, data);
    }

    /// @notice Collect fees by submitting a signed Receipt Aggregate Voucher (RAV).
    ///
    /// Flow: MCPDataService.collect() → GraphTallyCollector.collect()
    ///   → PaymentsEscrow.collect() → GraphPayments.collect()
    ///   → distributes: protocol tax → data service cut → delegator cut → provider.
    ///
    /// @param serviceProvider The provider collecting fees.
    /// @param paymentType Must be QueryFee.
    /// @param data ABI-encoded (SignedRAV, tokensToCollect).
    function collect(address serviceProvider, IGraphPayments.PaymentTypes paymentType, bytes calldata data)
        external
        override
        whenNotPaused
        returns (uint256 fees)
    {
        if (paymentType != IGraphPayments.PaymentTypes.QueryFee) revert InvalidPaymentType();
        if (!registeredProviders[serviceProvider]) revert ProviderNotRegistered(serviceProvider);

        (IGraphTallyCollector.SignedRAV memory signedRav, uint256 tokensToCollect) =
            abi.decode(data, (IGraphTallyCollector.SignedRAV, uint256));

        if (signedRav.rav.serviceProvider != serviceProvider) {
            revert InvalidServiceProvider(serviceProvider, signedRav.rav.serviceProvider);
        }

        _releaseStake(serviceProvider, 0);

        uint256 balanceBefore = _graphToken().balanceOf(address(this));
        fees = GRAPH_TALLY_COLLECTOR.collect(
            paymentType,
            abi.encode(signedRav, BURN_CUT_PPM + DATA_SERVICE_CUT_PPM, paymentsDestination[serviceProvider]),
            tokensToCollect
        );

        uint256 received = _graphToken().balanceOf(address(this)) - balanceBefore;
        if (received > 0) {
            uint256 burned = received * BURN_CUT_PPM / (BURN_CUT_PPM + DATA_SERVICE_CUT_PPM);
            _graphToken().burn(burned);
            emit FeesBurned(serviceProvider, burned);
        }

        if (fees > 0) {
            _lockStake(serviceProvider, fees * STAKE_TO_FEES_RATIO, block.timestamp + minThawingPeriod);
        }
    }

    /// @notice Slashing not implemented — this data service does not support on-chain dispute slashing.
    function slash(address, bytes calldata) external pure override {
        revert("slashing not supported");
    }

    /// @notice Accept pending provision parameter changes for this provider.
    function acceptProvisionPendingParameters(address serviceProvider, bytes calldata)
        external
        override
        onlyAuthorizedForProvision(serviceProvider)
    {
        _acceptProvisionParameters(serviceProvider);
    }

    // -------------------------------------------------------------------------
    // Views
    // -------------------------------------------------------------------------

    /// @inheritdoc IMCPDataService
    function isRegistered(address provider) external view override returns (bool) {
        return registeredProviders[provider];
    }

    /// @inheritdoc IMCPDataService
    function getSubgraphs(address provider)
        external
        view
        override
        returns (SubgraphEntry[] memory)
    {
        return _providerSubgraphs[provider];
    }

    /// @inheritdoc IMCPDataService
    function activeSubgraphCount(address provider) public view override returns (uint256 count) {
        SubgraphEntry[] storage entries = _providerSubgraphs[provider];
        for (uint256 i = 0; i < entries.length; i++) {
            if (entries[i].active) count++;
        }
    }

    /// @notice Grant or revoke pause guardian status.
    function setPauseGuardian(address guardian, bool allowed) external onlyOwner {
        _setPauseGuardian(guardian, allowed);
    }

    /// @notice Withdraw accumulated data service revenue to `to`.
    function withdrawFees(address to, uint256 amount) external onlyOwner {
        require(to != address(0), "zero address");
        _graphToken().transfer(to, amount);
        emit FeesWithdrawn(to, amount);
    }
}
