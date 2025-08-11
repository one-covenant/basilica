// SPDX-License-Identifier: UNLICENSED

pragma solidity ^0.8.28;

import {Test, console} from "forge-std/Test.sol";
import {CollateralUpgradeable} from "../src/CollateralUpgradeable.sol";
import {CollateralUpgradeableV2} from "../src/CollateralUpgradeableV2.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

contract CollateralUpgradeableTest is Test {
    CollateralUpgradeable public collateral;
    CollateralUpgradeable public implementation;
    ERC1967Proxy public proxy;

    // Test parameters
    uint16 constant NETUID = 42;
    address constant trustee = address(0x123);
    uint256 constant MIN_DEPOSIT = 1 ether;
    uint64 constant DECISION_TIMEOUT = 3600; // 1 hour
    address constant admin = address(0x456);
    address constant alice = address(0x789);

    function setUp() public {
        // Deploy implementation
        implementation = new CollateralUpgradeable();

        // Prepare initialization data
        bytes memory initData = abi.encodeWithSelector(
            CollateralUpgradeable.initialize.selector,
            NETUID,
            trustee,
            MIN_DEPOSIT,
            DECISION_TIMEOUT,
            admin
        );

        // Deploy proxy
        proxy = new ERC1967Proxy(address(implementation), initData);

        // Cast proxy to interface
        collateral = CollateralUpgradeable(payable(address(proxy)));
    }

    /// @dev Test basic initialization
    function testInitialization() public view {
        assertEq(collateral.NETUID(), NETUID);
        assertEq(collateral.TRUSTEE(), trustee);
        assertEq(collateral.MIN_COLLATERAL_INCREASE(), MIN_DEPOSIT);
        assertEq(collateral.DECISION_TIMEOUT(), DECISION_TIMEOUT);
        assertEq(collateral.getVersion(), 1);

        // Check roles
        assertTrue(collateral.hasRole(collateral.DEFAULT_ADMIN_ROLE(), admin));
        assertTrue(collateral.hasRole(collateral.UPGRADER_ROLE(), admin));
    }

    /// @dev Test that implementation cannot be initialized directly
    function testImplementationCannotBeInitialized() public {
        CollateralUpgradeable directImplementation = new CollateralUpgradeable();

        vm.expectRevert(); // Should revert due to _disableInitializers()
        directImplementation.initialize(
            NETUID,
            trustee,
            MIN_DEPOSIT,
            DECISION_TIMEOUT,
            admin
        );
    }

    /// @dev Test basic deposit functionality
    function testBasicDeposit() public {
        vm.deal(alice, 10 ether);
        bytes32 hotkey = bytes32(uint256(1));
        bytes16 executorId = bytes16(uint128(1));

        // Test event emission
        vm.expectEmit(true, true, true, true, address(collateral));
        emit Deposit(hotkey, executorId, alice, 5 ether);

        vm.prank(alice);
        collateral.deposit{value: 5 ether}(hotkey, executorId);

        // Verify state
        assertEq(collateral.collaterals(hotkey, executorId), 5 ether);
        assertEq(collateral.executorToMiner(hotkey, executorId), alice);
        assertEq(address(collateral).balance, 5 ether);
    }

    /// @dev Test admin functions
    function testAdminFunctions() public {
        address newTrustee = makeAddr("newTrustee");

        // Test trustee update
        vm.expectEmit(true, true, false, false, address(collateral));
        emit TrusteeUpdated(trustee, newTrustee);

        vm.prank(admin);
        collateral.updateTrustee(newTrustee);
        assertEq(collateral.TRUSTEE(), newTrustee);

        // Test decision timeout update
        vm.prank(admin);
        collateral.updateDecisionTimeout(7200); // 2 hours
        assertEq(collateral.DECISION_TIMEOUT(), 7200);

        // Test min collateral increase update
        vm.prank(admin);
        collateral.updateMinCollateralIncrease(2 ether);
        assertEq(collateral.MIN_COLLATERAL_INCREASE(), 2 ether);
    }

    /// @dev Test contract upgrade functionality
    function testUpgrade() public {
        // Deploy new implementation
        CollateralUpgradeableV2 newImplementation = new CollateralUpgradeableV2();

        // Test event emission
        vm.expectEmit(true, true, false, false, address(collateral));
        emit ContractUpgraded(2, address(newImplementation));

        // Upgrade to new implementation
        vm.prank(admin);
        collateral.upgradeToAndCall(address(newImplementation), "");

        // Verify upgrade
        assertEq(collateral.getVersion(), 2);

        // Verify state is preserved
        assertEq(collateral.NETUID(), NETUID);
        assertEq(collateral.TRUSTEE(), trustee);
        assertEq(collateral.DECISION_TIMEOUT(), DECISION_TIMEOUT);
        assertEq(collateral.MIN_COLLATERAL_INCREASE(), MIN_DEPOSIT);

        // Verify admin still has roles
        assertTrue(collateral.hasRole(collateral.DEFAULT_ADMIN_ROLE(), admin));
        assertTrue(collateral.hasRole(collateral.UPGRADER_ROLE(), admin));
    }

    event Deposit(
        bytes32 indexed hotkey,
        bytes16 indexed executorId,
        address indexed miner,
        uint256 amount
    );
    event ReclaimProcessStarted(
        uint256 indexed reclaimRequestId,
        bytes32 indexed hotkey,
        bytes16 indexed executorId,
        address miner,
        uint256 amount,
        uint64 expirationTime,
        string url,
        bytes16 urlContentMd5Checksum
    );
    event Reclaimed(
        uint256 indexed reclaimRequestId,
        bytes32 indexed hotkey,
        bytes16 indexed executorId,
        address miner,
        uint256 amount
    );
    event Denied(
        uint256 indexed reclaimRequestId,
        string url,
        bytes16 urlContentMd5Checksum
    );
    event Slashed(
        bytes32 indexed hotkey,
        bytes16 indexed executorId,
        address indexed miner,
        uint256 amount,
        string url,
        bytes16 urlContentMd5Checksum
    );

    // Upgrade event
    event ContractUpgraded(
        uint256 indexed newVersion,
        address indexed newImplementation
    );

    event TrusteeUpdated(
        address indexed oldTrustee,
        address indexed newTrustee
    );
}
