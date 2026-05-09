// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {Script, console} from "forge-std/Script.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {MCPDataService} from "../src/MCPDataService.sol";

/// @notice Deploy MCPDataService as a UUPS proxy on Arbitrum One or Arbitrum Sepolia.
///
/// Usage:
///   forge script contracts/script/Deploy.s.sol \
///     --rpc-url arbitrum_sepolia \
///     --broadcast \
///     --verify \
///     -vvvv
///
/// Required env vars:
///   DEPLOYER_PRIVATE_KEY      — deployer / owner key
///   GRAPH_CONTROLLER          — Graph Protocol controller address
///   GRAPH_TALLY_COLLECTOR     — GraphTallyCollector contract address
///   PAUSE_GUARDIAN            — address authorised to pause (can be deployer for testnet)
contract Deploy is Script {
    // Arbitrum One: 0x454bB3cb427B21e1c052A080e21A57753cd6969a (from graph-network-arbitrum)
    // Arbitrum Sepolia: check The Graph's official deployment manifest.
    address constant CONTROLLER_MAINNET    = 0x454bb3cb427B21E1C052A080e21a57753cD6969A;
    address constant TALLY_COLLECTOR_MAINNET = 0x8f69F5C07477Ac46FBc491B1E6D91E2bb0111A9e;

    function run() external {
        uint256 deployerKey = vm.envUint("DEPLOYER_PRIVATE_KEY");
        address deployer = vm.addr(deployerKey);

        address controller = vm.envOr("GRAPH_CONTROLLER", CONTROLLER_MAINNET);
        address tallyCollector = vm.envOr("GRAPH_TALLY_COLLECTOR", TALLY_COLLECTOR_MAINNET);
        address pauseGuardian = vm.envOr("PAUSE_GUARDIAN", deployer);

        vm.startBroadcast(deployerKey);

        MCPDataService impl = new MCPDataService(controller, tallyCollector);
        console.log("MCPDataService implementation:", address(impl));

        bytes memory initData = abi.encodeCall(MCPDataService.initialize, (deployer, pauseGuardian));
        ERC1967Proxy proxy = new ERC1967Proxy(address(impl), initData);
        console.log("MCPDataService proxy (use this address):", address(proxy));

        vm.stopBroadcast();
    }
}
