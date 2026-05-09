// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {Test, console} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

// TODO: wire up Horizon mock infrastructure (same pattern as dispatch-tap tests).
// For now: placeholder to keep `forge build` green.

contract MCPDataServiceTest is Test {
    function test_placeholder() public pure {
        // Week 1 TODO: deploy with HorizonMocks, test register + addSubgraph + collect.
        assertTrue(true);
    }
}
