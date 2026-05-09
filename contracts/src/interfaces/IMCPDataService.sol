// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

/// @title IMCPDataService
/// @notice Interface for the Compass MCP data service built on The Graph Protocol's Horizon framework.
///
/// Indexers provision GRT into this contract, register themselves, then add
/// each subgraph deployment they wish to serve as an MCP tool. Agents pay
/// per-call via GraphTally (TAP v2) receipts; indexers collect fees by
/// submitting signed RAVs to collect().
interface IMCPDataService {
    // -------------------------------------------------------------------------
    // Errors
    // -------------------------------------------------------------------------

    error ProviderAlreadyRegistered(address provider);
    error ProviderNotRegistered(address provider);
    error ActiveSubgraphsExist(address provider);
    error SubgraphNotFound(address provider, bytes32 deploymentId);
    error SubgraphAlreadyActive(address provider, bytes32 deploymentId);
    error InvalidPaymentType();
    error InvalidServiceProvider(address expected, address got);
    error ThawingPeriodTooShort(uint64 minimum, uint64 given);

    // -------------------------------------------------------------------------
    // Events
    // -------------------------------------------------------------------------

    event ProviderRegistered(address indexed provider, string endpoint);
    event ProviderDeregistered(address indexed provider);
    event SubgraphAdded(address indexed provider, bytes32 indexed deploymentId, uint128 pricePerQuery);
    event SubgraphRemoved(address indexed provider, bytes32 indexed deploymentId);
    event PaymentsDestinationSet(address indexed provider, address indexed destination);
    event FeesBurned(address indexed provider, uint256 amount);
    event FeesWithdrawn(address indexed to, uint256 amount);
    event MinThawingPeriodSet(uint64 period);

    // -------------------------------------------------------------------------
    // Structs
    // -------------------------------------------------------------------------

    /// A subgraph deployment registered for MCP serving by a provider.
    struct SubgraphEntry {
        /// IPFS deployment hash (keccak256 of the CID multihash bytes, matching SubgraphService convention).
        bytes32 deploymentId;
        /// Price per MCP tool call in GRT wei.
        uint128 pricePerQuery;
        /// MCP endpoint URL for this provider's Compass gateway.
        string endpoint;
        bool active;
    }

    // -------------------------------------------------------------------------
    // Mutating functions
    // -------------------------------------------------------------------------

    /// Add a subgraph deployment to this provider's MCP service.
    /// @param serviceProvider The provider's staking address.
    /// @param data ABI-encoded (bytes32 deploymentId, uint128 pricePerQuery, string endpoint).
    function addSubgraph(address serviceProvider, bytes calldata data) external;

    /// Remove a subgraph deployment from this provider's MCP service.
    /// @param serviceProvider The provider's staking address.
    /// @param data ABI-encoded (bytes32 deploymentId).
    function removeSubgraph(address serviceProvider, bytes calldata data) external;

    /// Set the address that receives collected GRT for this provider.
    function setPaymentsDestination(address destination) external;

    /// Governance: lower-bound the thawing period.
    function setMinThawingPeriod(uint64 period) external;

    // -------------------------------------------------------------------------
    // Views
    // -------------------------------------------------------------------------

    function isRegistered(address provider) external view returns (bool);
    function getSubgraphs(address provider) external view returns (SubgraphEntry[] memory);
    function activeSubgraphCount(address provider) external view returns (uint256);
}
