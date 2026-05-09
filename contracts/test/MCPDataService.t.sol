// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

import {MCPDataService} from "../src/MCPDataService.sol";
import {IMCPDataService} from "../src/interfaces/IMCPDataService.sol";
import {IHorizonStakingTypes} from "@graphprotocol/interfaces/contracts/horizon/internal/IHorizonStakingTypes.sol";

// -------------------------------------------------------------------------
// Mocks
// -------------------------------------------------------------------------

/// @dev Minimal mock of IHorizonStaking — only the provision-related methods
///      used by MCPDataService / DataService.
contract MockHorizonStaking {
    mapping(address => mapping(address => IHorizonStakingTypes.Provision)) public provisions;

    function setProvision(address serviceProvider, address dataService, uint256 tokens, uint64 thawingPeriod_)
        external
    {
        provisions[serviceProvider][dataService] = IHorizonStakingTypes.Provision({
            tokens: tokens,
            tokensThawing: 0,
            sharesThawing: 0,
            maxVerifierCut: 1_000_000,
            thawingPeriod: thawingPeriod_,
            createdAt: uint64(block.timestamp),
            maxVerifierCutPending: 0,
            thawingPeriodPending: 0,
            lastParametersStagedAt: 0,
            thawingNonce: 0
        });
    }

    function getProvision(address serviceProvider, address dataService)
        external
        view
        returns (IHorizonStakingTypes.Provision memory)
    {
        return provisions[serviceProvider][dataService];
    }

    function isAuthorized(address serviceProvider, address, address operator) external pure returns (bool) {
        return serviceProvider == operator;
    }

    function slash(address, uint256, uint256, address) external {}
    function acceptProvisionParameters(address) external {}
}

/// @dev Mock IController — returns the staking mock address for "Staking",
///      address(1) for everything else that GraphDirectory asks for.
contract MockController {
    mapping(bytes32 => address) private _contracts;

    constructor(address staking_) {
        address dummy = address(1);
        _contracts[keccak256("GraphToken")]        = dummy;
        _contracts[keccak256("Staking")]           = staking_;
        _contracts[keccak256("GraphPayments")]     = dummy;
        _contracts[keccak256("PaymentsEscrow")]    = dummy;
        _contracts[keccak256("EpochManager")]      = dummy;
        _contracts[keccak256("RewardsManager")]    = dummy;
        _contracts[keccak256("GraphTokenGateway")] = dummy;
        _contracts[keccak256("GraphProxyAdmin")]   = dummy;
        _contracts[keccak256("Curation")]          = dummy;
    }

    function getContractProxy(bytes32 id) external view returns (address) {
        return _contracts[id];
    }
}

// -------------------------------------------------------------------------
// Test suite
// -------------------------------------------------------------------------

contract MCPDataServiceTest is Test {
    MCPDataService public service;
    MockHorizonStaking public staking;
    MockController public controller;

    address public owner         = makeAddr("owner");
    address public pauseGuardian = makeAddr("pauseGuardian");
    address public provider      = makeAddr("provider");

    uint256 constant SUFFICIENT_PROVISION = 10_000e18;
    uint64  constant SUFFICIENT_THAWING   = 14 days;

    bytes32 constant DEPLOYMENT_ENS     = keccak256("ens-mainnet");
    bytes32 constant DEPLOYMENT_UNISWAP = keccak256("uniswap-v3-mainnet");
    uint128 constant PRICE_PER_QUERY    = 10_000_000_000_000; // 0.00001 GRT
    string  constant MCP_ENDPOINT       = "https://compass.example.com";

    function setUp() public {
        staking    = new MockHorizonStaking();
        controller = new MockController(address(staking));

        MCPDataService impl = new MCPDataService(address(controller), address(0));
        service = MCPDataService(
            address(
                new ERC1967Proxy(
                    address(impl),
                    abi.encodeCall(MCPDataService.initialize, (owner, pauseGuardian))
                )
            )
        );

        // Pre-populate staking mock with a valid provision for `provider`.
        staking.setProvision(provider, address(service), SUFFICIENT_PROVISION, SUFFICIENT_THAWING);
    }

    // -------------------------------------------------------------------------
    // register
    // -------------------------------------------------------------------------

    function test_register_succeeds() public {
        _register(provider, MCP_ENDPOINT, address(0));
        assertTrue(service.isRegistered(provider));
    }

    function test_register_defaultsPaymentsDestinationToProvider() public {
        _register(provider, MCP_ENDPOINT, address(0));
        assertEq(service.paymentsDestination(provider), provider);
    }

    function test_register_setsCustomPaymentsDestination() public {
        address wallet = makeAddr("paymentWallet");
        _register(provider, MCP_ENDPOINT, wallet);
        assertEq(service.paymentsDestination(provider), wallet);
    }

    function test_register_emitsEvent() public {
        vm.expectEmit(true, false, false, true);
        emit IMCPDataService.ProviderRegistered(provider, MCP_ENDPOINT);
        _register(provider, MCP_ENDPOINT, address(0));
    }

    function test_register_revertIfAlreadyRegistered() public {
        _register(provider, MCP_ENDPOINT, address(0));
        vm.expectRevert(abi.encodeWithSelector(IMCPDataService.ProviderAlreadyRegistered.selector, provider));
        _register(provider, MCP_ENDPOINT, address(0));
    }

    function test_register_revertIfInsufficientProvision() public {
        address poor = makeAddr("poor");
        staking.setProvision(poor, address(service), service.DEFAULT_MIN_PROVISION() - 1, SUFFICIENT_THAWING);
        vm.prank(poor);
        vm.expectRevert();
        service.register(poor, abi.encode(MCP_ENDPOINT, address(0)));
    }

    function test_register_revertIfThawingPeriodTooShort() public {
        address short_ = makeAddr("short");
        staking.setProvision(short_, address(service), SUFFICIENT_PROVISION, SUFFICIENT_THAWING - 1);
        vm.prank(short_);
        vm.expectRevert();
        service.register(short_, abi.encode(MCP_ENDPOINT, address(0)));
    }

    // -------------------------------------------------------------------------
    // setPaymentsDestination
    // -------------------------------------------------------------------------

    function test_setPaymentsDestination_updatesDestination() public {
        _register(provider, MCP_ENDPOINT, address(0));
        address newWallet = makeAddr("newWallet");
        vm.prank(provider);
        service.setPaymentsDestination(newWallet);
        assertEq(service.paymentsDestination(provider), newWallet);
    }

    function test_setPaymentsDestination_zeroResetsToSelf() public {
        address wallet = makeAddr("wallet");
        _register(provider, MCP_ENDPOINT, wallet);
        vm.prank(provider);
        service.setPaymentsDestination(address(0));
        assertEq(service.paymentsDestination(provider), provider);
    }

    function test_setPaymentsDestination_emitsEvent() public {
        _register(provider, MCP_ENDPOINT, address(0));
        address newWallet = makeAddr("newWallet");
        vm.expectEmit(true, true, false, false);
        emit IMCPDataService.PaymentsDestinationSet(provider, newWallet);
        vm.prank(provider);
        service.setPaymentsDestination(newWallet);
    }

    function test_setPaymentsDestination_revertIfNotRegistered() public {
        vm.prank(provider);
        vm.expectRevert(abi.encodeWithSelector(IMCPDataService.ProviderNotRegistered.selector, provider));
        service.setPaymentsDestination(makeAddr("wallet"));
    }

    // -------------------------------------------------------------------------
    // addSubgraph
    // -------------------------------------------------------------------------

    function test_addSubgraph_succeeds() public {
        _register(provider, MCP_ENDPOINT, address(0));
        _addSubgraph(provider, DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT);

        IMCPDataService.SubgraphEntry[] memory subs = service.getSubgraphs(provider);
        assertEq(subs.length, 1);
        assertEq(subs[0].deploymentId, DEPLOYMENT_ENS);
        assertEq(subs[0].pricePerQuery, PRICE_PER_QUERY);
        assertTrue(subs[0].active);
        assertEq(service.activeSubgraphCount(provider), 1);
    }

    function test_addSubgraph_multipleSubgraphs() public {
        _register(provider, MCP_ENDPOINT, address(0));
        _addSubgraph(provider, DEPLOYMENT_ENS,     PRICE_PER_QUERY,     MCP_ENDPOINT);
        _addSubgraph(provider, DEPLOYMENT_UNISWAP, PRICE_PER_QUERY * 2, MCP_ENDPOINT);

        assertEq(service.activeSubgraphCount(provider), 2);
    }

    function test_addSubgraph_emitsEvent() public {
        _register(provider, MCP_ENDPOINT, address(0));
        vm.expectEmit(true, true, false, true);
        emit IMCPDataService.SubgraphAdded(provider, DEPLOYMENT_ENS, PRICE_PER_QUERY);
        _addSubgraph(provider, DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT);
    }

    function test_addSubgraph_revertIfNotRegistered() public {
        vm.prank(provider);
        vm.expectRevert(abi.encodeWithSelector(IMCPDataService.ProviderNotRegistered.selector, provider));
        service.addSubgraph(provider, abi.encode(DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT));
    }

    function test_addSubgraph_revertIfAlreadyActive() public {
        _register(provider, MCP_ENDPOINT, address(0));
        _addSubgraph(provider, DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT);

        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(IMCPDataService.SubgraphAlreadyActive.selector, provider, DEPLOYMENT_ENS)
        );
        service.addSubgraph(provider, abi.encode(DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT));
    }

    function test_addSubgraph_reactivatesRemovedEntry() public {
        _register(provider, MCP_ENDPOINT, address(0));
        _addSubgraph(provider, DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT);
        _removeSubgraph(provider, DEPLOYMENT_ENS);

        // Re-add with a new price.
        uint128 newPrice = PRICE_PER_QUERY * 3;
        _addSubgraph(provider, DEPLOYMENT_ENS, newPrice, MCP_ENDPOINT);

        IMCPDataService.SubgraphEntry[] memory subs = service.getSubgraphs(provider);
        assertEq(subs.length, 1, "should reuse slot, not push a duplicate");
        assertTrue(subs[0].active);
        assertEq(subs[0].pricePerQuery, newPrice);
    }

    // -------------------------------------------------------------------------
    // removeSubgraph
    // -------------------------------------------------------------------------

    function test_removeSubgraph_deactivates() public {
        _register(provider, MCP_ENDPOINT, address(0));
        _addSubgraph(provider, DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT);
        _removeSubgraph(provider, DEPLOYMENT_ENS);

        IMCPDataService.SubgraphEntry[] memory subs = service.getSubgraphs(provider);
        assertFalse(subs[0].active);
        assertEq(service.activeSubgraphCount(provider), 0);
    }

    function test_removeSubgraph_emitsEvent() public {
        _register(provider, MCP_ENDPOINT, address(0));
        _addSubgraph(provider, DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT);
        vm.expectEmit(true, true, false, false);
        emit IMCPDataService.SubgraphRemoved(provider, DEPLOYMENT_ENS);
        _removeSubgraph(provider, DEPLOYMENT_ENS);
    }

    function test_removeSubgraph_revertIfNotFound() public {
        _register(provider, MCP_ENDPOINT, address(0));
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(IMCPDataService.SubgraphNotFound.selector, provider, DEPLOYMENT_ENS)
        );
        service.removeSubgraph(provider, abi.encode(DEPLOYMENT_ENS));
    }

    // -------------------------------------------------------------------------
    // deregister
    // -------------------------------------------------------------------------

    function test_deregister_succeedsWhenNoActiveSubgraphs() public {
        _register(provider, MCP_ENDPOINT, address(0));
        vm.prank(provider);
        service.deregister(provider, "");
        assertFalse(service.isRegistered(provider));
    }

    function test_deregister_revertIfActiveSubgraphsExist() public {
        _register(provider, MCP_ENDPOINT, address(0));
        _addSubgraph(provider, DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT);
        vm.prank(provider);
        vm.expectRevert(abi.encodeWithSelector(IMCPDataService.ActiveSubgraphsExist.selector, provider));
        service.deregister(provider, "");
    }

    function test_deregister_succeeds_afterRemovingSubgraphs() public {
        _register(provider, MCP_ENDPOINT, address(0));
        _addSubgraph(provider, DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT);
        _removeSubgraph(provider, DEPLOYMENT_ENS);
        vm.prank(provider);
        service.deregister(provider, "");
        assertFalse(service.isRegistered(provider));
    }

    // -------------------------------------------------------------------------
    // setMinThawingPeriod
    // -------------------------------------------------------------------------

    function test_setMinThawingPeriod_storesPeriod() public {
        uint64 newPeriod = 28 days;
        vm.prank(owner);
        service.setMinThawingPeriod(newPeriod);
        assertEq(service.minThawingPeriod(), newPeriod);
    }

    function test_setMinThawingPeriod_emitsEvent() public {
        uint64 newPeriod = 21 days;
        vm.expectEmit(false, false, false, true);
        emit IMCPDataService.MinThawingPeriodSet(newPeriod);
        vm.prank(owner);
        service.setMinThawingPeriod(newPeriod);
    }

    function test_setMinThawingPeriod_revertIfTooShort() public {
        vm.prank(owner);
        vm.expectRevert(
            abi.encodeWithSelector(IMCPDataService.ThawingPeriodTooShort.selector, uint64(14 days), uint64(13 days))
        );
        service.setMinThawingPeriod(13 days);
    }

    function test_setMinThawingPeriod_revertIfNotOwner() public {
        vm.prank(makeAddr("attacker"));
        vm.expectRevert();
        service.setMinThawingPeriod(28 days);
    }

    function test_minThawingPeriod_initializedToConstant() public view {
        assertEq(service.minThawingPeriod(), service.MIN_THAWING_PERIOD());
    }

    // -------------------------------------------------------------------------
    // Pause
    // -------------------------------------------------------------------------

    function test_pause_blocksRegister() public {
        vm.prank(pauseGuardian);
        service.pause();
        vm.prank(provider);
        vm.expectRevert();
        service.register(provider, abi.encode(MCP_ENDPOINT, address(0)));
    }

    function test_pause_blocksAddSubgraph() public {
        _register(provider, MCP_ENDPOINT, address(0));
        vm.prank(pauseGuardian);
        service.pause();
        vm.prank(provider);
        vm.expectRevert();
        service.addSubgraph(provider, abi.encode(DEPLOYMENT_ENS, PRICE_PER_QUERY, MCP_ENDPOINT));
    }

    function test_unpause_allowsRegister() public {
        vm.prank(pauseGuardian);
        service.pause();
        vm.prank(pauseGuardian);
        service.unpause();
        _register(provider, MCP_ENDPOINT, address(0));
        assertTrue(service.isRegistered(provider));
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    function _register(address _provider, string memory endpoint, address dest) internal {
        vm.prank(_provider);
        service.register(_provider, abi.encode(endpoint, dest));
    }

    function _addSubgraph(address _provider, bytes32 deploymentId, uint128 price, string memory endpoint) internal {
        vm.prank(_provider);
        service.addSubgraph(_provider, abi.encode(deploymentId, price, endpoint));
    }

    function _removeSubgraph(address _provider, bytes32 deploymentId) internal {
        vm.prank(_provider);
        service.removeSubgraph(_provider, abi.encode(deploymentId));
    }
}
