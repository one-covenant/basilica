// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

import {Test, console, Vm} from "forge-std/Test.sol";
import {Collateral} from "../src/Collateral.sol";

contract CollateralTestnetTest is Test {
    Collateral public collateral;

    // Testnet-specific addresses
    address constant DEPLOYED_CONTRACT = address(0); // Set this after deployment
    address constant TESTNET_USER = 0x70997970C51812dc3A010C7d01b50e0d17dc79C8;
    address constant TESTNET_TRUSTEE =
        0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC;

    uint256 constant FORK_BLOCK = 4000000; // Adjust based on your needs

    function setUp() public {
        // Option 1: Deploy new contract for testing
        deployFreshContract();

        // Option 2: Use existing deployed contract
        // useDeployedContract();
    }

    function deployFreshContract() internal {
        uint16 netuid = 42; // Match your config
        uint256 minCollateralIncrease = 1 ether;
        uint64 decisionTimeout = 3600; // 1 hour

        collateral = new Collateral(
            netuid,
            TESTNET_TRUSTEE,
            minCollateralIncrease,
            decisionTimeout
        );

        console.log("Deployed Collateral at:", address(collateral));
    }

    function useDeployedContract() internal {
        require(
            DEPLOYED_CONTRACT != address(0),
            "Set DEPLOYED_CONTRACT address"
        );
        collateral = Collateral(payable(DEPLOYED_CONTRACT));

        // Verify contract is working
        uint16 netuid = collateral.NETUID();
        console.log("Using deployed contract, NETUID:", netuid);
    }

    function testDepositOnTestnet() public {
        // Use realistic testnet addresses
        address user = TESTNET_USER;
        uint256 depositAmount = 2 ether;

        // Fund the user (only works on fork, not live testnet)
        vm.deal(user, depositAmount);

        // Test event emission for deposit
        vm.expectEmit(true, true, true, true, address(collateral));
        emit Deposit(bytes16(uint128(1)), user, depositAmount);

        // Perform deposit
        vm.prank(user);
        collateral.deposit{value: depositAmount}(bytes16(uint128(1)));

        // Verify deposit
        assertEq(collateral.collaterals(bytes16(uint128(1))), depositAmount);
        assertEq(collateral.executorToMiner(bytes16(uint128(1))), user);

        console.log("Deposit successful on testnet");
        console.log("Contract balance:", address(collateral).balance);
    }

    // ============ Event Testing Examples ============

    /// @dev Example 1: Check all parameters
    function testEventCheckAll() public {
        address user = makeAddr("testUser");
        vm.deal(user, 10 ether);

        // Check all indexed parameters AND data
        vm.expectEmit(true, true, true, true, address(collateral));
        emit Deposit(bytes16(uint128(99)), user, 3 ether);

        vm.prank(user);
        collateral.deposit{value: 3 ether}(bytes16(uint128(99)));
    }

    /// @dev Example 2: Check only indexed parameters (ignore data)
    function testEventCheckIndexedOnly() public {
        address user = makeAddr("testUser");
        vm.deal(user, 10 ether);

        // Check indexed parameters but ignore amount (data)
        vm.expectEmit(true, true, false, false, address(collateral));
        emit Deposit(bytes16(uint128(99)), user, 0); // amount doesn't matter

        vm.prank(user);
        collateral.deposit{value: 3 ether}(bytes16(uint128(99)));
    }

    /// @dev Example 4: Test without specifying emitter (any contract can emit)
    function testEventAnyEmitter() public {
        address user = makeAddr("testUser");
        vm.deal(user, 10 ether);

        // Don't specify emitter address
        vm.expectEmit(true, true, true, true);
        emit Deposit(bytes16(uint128(99)), user, 3 ether);

        vm.prank(user);
        collateral.deposit{value: 3 ether}(bytes16(uint128(99)));
    }

    event Deposit(
        bytes16 indexed executorId,
        address indexed miner,
        uint256 amount
    );
    event ReclaimProcessStarted(
        uint256 indexed reclaimRequestId,
        bytes16 indexed executorId,
        address indexed miner,
        uint256 amount,
        uint64 expirationTime,
        string url,
        bytes16 urlContentMd5Checksum
    );
    event Reclaimed(
        uint256 indexed reclaimRequestId,
        bytes16 indexed executorId,
        address indexed miner,
        uint256 amount
    );
    event Denied(
        uint256 indexed reclaimRequestId,
        string url,
        bytes16 urlContentMd5Checksum
    );
    event Slashed(
        bytes16 indexed executorId,
        address indexed miner,
        uint256 amount,
        string url,
        bytes16 urlContentMd5Checksum
    );
}
