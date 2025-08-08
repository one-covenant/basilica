// SPDX-License-Identifier: UNLICENSED
// The contract is the same as CollateralUpgradeable.sol, just different version number.
// Only for testing purposes.

pragma solidity ^0.8.28;

import "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";
import "@openzeppelin/contracts-upgradeable/access/AccessControlUpgradeable.sol";

contract CollateralUpgradeableV2 is
    Initializable,
    UUPSUpgradeable,
    AccessControlUpgradeable
{
    constructor() {
        _disableInitializers();
    }

    // Version for tracking upgrades
    function getVersion() external pure virtual returns (uint256) {
        return 2;
    }

    // Role for upgrading the contract
    bytes32 public constant UPGRADER_ROLE = keccak256("UPGRADER_ROLE");

    // State variables
    uint16 public NETUID;
    address public TRUSTEE;
    uint64 public DECISION_TIMEOUT;
    uint256 public MIN_COLLATERAL_INCREASE;

    mapping(bytes32 => mapping(bytes16 => address)) public executorToMiner;
    mapping(bytes32 => mapping(bytes16 => uint256)) public collaterals;
    mapping(uint256 => Reclaim) public reclaims;

    mapping(bytes32 => mapping(bytes16 => uint256))
        private collateralUnderPendingReclaims;
    uint256 private nextReclaimId;

    struct Reclaim {
        bytes32 hotkey;
        bytes16 executorId;
        address miner;
        uint256 amount;
        uint64 denyTimeout;
    }

    // Events
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

    // Custom errors
    error AmountZero();
    error BeforeDenyTimeout();
    error ExecutorNotOwned();
    error InsufficientAmount();
    error InvalidDepositMethod();
    error NotTrustee();
    error PastDenyTimeout();
    error ReclaimNotFound();
    error TransferFailed();
    error InsufficientCollateralForReclaim();

    function initializeV2() external {}

    modifier onlyTrustee() {
        if (msg.sender != TRUSTEE) {
            revert NotTrustee();
        }
        _;
    }

    // Allow deposits only via deposit() function
    receive() external payable {
        revert InvalidDepositMethod();
    }

    // Allow deposits only via deposit() function
    fallback() external payable {
        revert InvalidDepositMethod();
    }

    /// @notice Allows users to deposit collateral into the contract for a specific executor
    /// @param hotkey The netuid key for the subnet
    /// @param executorId The ID of the executor to deposit collateral for
    /// @dev The first deposit for an executorId sets the owner. Subsequent deposits must be from the owner.
    /// @dev The deposited amount must be greater than or equal to MIN_COLLATERAL_INCREASE
    /// @dev Emits a Deposit event with the hotkey, executorId, sender's address and deposited amount
    function deposit(
        bytes32 hotkey,
        bytes16 executorId
    ) external payable virtual {
        if (msg.value < MIN_COLLATERAL_INCREASE) {
            revert InsufficientAmount();
        }

        address owner = executorToMiner[hotkey][executorId];
        if (owner == address(0)) {
            executorToMiner[hotkey][executorId] = msg.sender;
        } else if (owner != msg.sender) {
            revert ExecutorNotOwned();
        }

        collaterals[hotkey][executorId] += msg.value;
        emit Deposit(hotkey, executorId, msg.sender, msg.value);
    }

    /// @notice Initiates a process to reclaim all available collateral from a specific executor
    /// @dev If it's not denied by the trustee, the collateral will be available for withdrawal after DECISION_TIMEOUT
    /// @param hotkey The netuid key for the subnet
    /// @param executorId The ID of the executor to reclaim collateral from
    /// @param url URL containing information about the reclaim request
    /// @param urlContentMd5Checksum MD5 checksum of the content at the provided URL
    /// @dev Emits ReclaimProcessStarted event with reclaim details and timeout
    /// @dev Reverts with ExecutorNotOwned if caller is not the owner of the executor
    /// @dev Reverts with AmountZero if there is no available collateral to reclaim
    function reclaimCollateral(
        bytes32 hotkey,
        bytes16 executorId,
        string calldata url,
        bytes16 urlContentMd5Checksum
    ) external {
        if (msg.sender != executorToMiner[hotkey][executorId]) {
            revert ExecutorNotOwned();
        }

        uint256 totalCollateral = collaterals[hotkey][executorId];
        uint256 pendingCollateral = collateralUnderPendingReclaims[hotkey][
            executorId
        ];
        uint256 availableAmount = totalCollateral - pendingCollateral;

        if (availableAmount == 0) {
            revert AmountZero();
        }

        uint64 denyTimeout = uint64(block.timestamp) + DECISION_TIMEOUT;

        reclaims[nextReclaimId] = Reclaim({
            hotkey: hotkey,
            executorId: executorId,
            miner: msg.sender,
            amount: availableAmount,
            denyTimeout: denyTimeout
        });

        collateralUnderPendingReclaims[hotkey][executorId] += availableAmount;

        emit ReclaimProcessStarted(
            nextReclaimId,
            hotkey,
            executorId,
            msg.sender,
            availableAmount,
            denyTimeout,
            url,
            urlContentMd5Checksum
        );

        nextReclaimId++;
    }

    /// @notice Finalizes a reclaim request after the deny timeout has expired
    /// @dev Can only be called after the deny timeout has passed for the specific reclaim request
    /// @dev Transfers the collateral to the miner and removes the executor-to-miner mapping if successful
    /// @dev This fully closes the relationship, allowing to request another reclaim
    /// @param reclaimRequestId The ID of the reclaim request to finalize
    /// @dev Emits Reclaimed event with reclaim details if successful
    /// @dev Reverts with ReclaimNotFound if the reclaim request doesn't exist or was denied
    /// @dev Reverts with BeforeDenyTimeout if the deny timeout hasn't expired
    /// @dev Reverts with TransferFailed if the TAO transfer fails
    function finalizeReclaim(uint256 reclaimRequestId) external {
        Reclaim storage reclaim = reclaims[reclaimRequestId];
        if (reclaim.amount == 0) {
            revert ReclaimNotFound();
        }
        if (reclaim.denyTimeout >= block.timestamp) {
            revert BeforeDenyTimeout();
        }

        bytes32 hotkey = reclaim.hotkey;
        bytes16 executorId = reclaim.executorId;
        address miner = reclaim.miner;
        uint256 amount = reclaim.amount;

        delete reclaims[reclaimRequestId];
        collateralUnderPendingReclaims[hotkey][executorId] -= amount;

        if (collaterals[hotkey][executorId] < amount) {
            // miner got slashed and can't withdraw
            revert InsufficientCollateralForReclaim();
        }

        collaterals[hotkey][executorId] -= amount;

        emit Reclaimed(reclaimRequestId, hotkey, executorId, miner, amount);

        // check-effect-interact pattern used to prevent reentrancy attacks
        (bool success, ) = payable(miner).call{value: amount}("");
        if (!success) {
            revert TransferFailed();
        }
        executorToMiner[hotkey][executorId] = address(0);
    }

    /// @notice Allows the trustee to deny a pending reclaim request before the timeout expires
    /// @dev Can only be called by the trustee (address set in initializer)
    /// @dev Must be called before the deny timeout expires
    /// @dev Removes the reclaim request and frees up the collateral for other reclaims
    /// @param reclaimRequestId The ID of the reclaim request to deny
    /// @param url URL containing the reason of denial
    /// @param urlContentMd5Checksum MD5 checksum of the content at the provided URL
    /// @dev Emits Denied event with the reclaim request ID
    /// @dev Reverts with NotTrustee if called by non-trustee address
    /// @dev Reverts with ReclaimNotFound if the reclaim request doesn't exist
    /// @dev Reverts with PastDenyTimeout if the timeout has already expired
    function denyReclaimRequest(
        uint256 reclaimRequestId,
        string calldata url,
        bytes16 urlContentMd5Checksum
    ) external onlyTrustee {
        Reclaim storage reclaim = reclaims[reclaimRequestId];
        if (reclaim.amount == 0) {
            revert ReclaimNotFound();
        }
        if (reclaim.denyTimeout < block.timestamp) {
            revert PastDenyTimeout();
        }

        collateralUnderPendingReclaims[reclaim.hotkey][
            reclaim.executorId
        ] -= reclaim.amount;
        emit Denied(reclaimRequestId, url, urlContentMd5Checksum);

        delete reclaims[reclaimRequestId];
    }

    /// @notice Allows the trustee to slash a miner's collateral for a specific executor
    /// @dev Can only be called by the trustee (address set in initializer)
    /// @dev Removes the collateral from the executor and burns it
    /// @param hotkey The netuid key for the subnet
    /// @param executorId The ID of the executor to slash
    /// @param url URL containing the reason for slashing
    /// @param urlContentMd5Checksum MD5 checksum of the content at the provided URL
    /// @dev Emits Slashed event with the executor's ID, miner's address and the amount slashed
    /// @dev Reverts with AmountZero if there is no collateral to slash
    /// @dev Reverts with TransferFailed if the TAO transfer fails
    function slashCollateral(
        bytes32 hotkey,
        bytes16 executorId,
        string calldata url,
        bytes16 urlContentMd5Checksum
    ) external onlyTrustee {
        uint256 amount = collaterals[hotkey][executorId];

        if (amount == 0) {
            revert AmountZero();
        }

        collaterals[hotkey][executorId] = 0;
        address miner = executorToMiner[hotkey][executorId];

        // burn the collateral
        (bool success, ) = payable(address(0)).call{value: amount}("");
        if (!success) {
            revert TransferFailed();
        }
        executorToMiner[hotkey][executorId] = address(0);
        emit Slashed(
            hotkey,
            executorId,
            miner,
            amount,
            url,
            urlContentMd5Checksum
        );
    }

    /// @notice Updates the trustee address
    /// @param newTrustee The new trustee address
    /// @dev Can only be called by accounts with DEFAULT_ADMIN_ROLE
    function updateTrustee(
        address newTrustee
    ) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(newTrustee != address(0), "New trustee cannot be zero address");
        address oldTrustee = TRUSTEE;
        TRUSTEE = newTrustee;

        // Emit an event for the trustee change
        emit TrusteeUpdated(oldTrustee, newTrustee);
    }

    /// @notice Updates the decision timeout
    /// @param newTimeout The new decision timeout in seconds
    /// @dev Can only be called by accounts with DEFAULT_ADMIN_ROLE
    function updateDecisionTimeout(
        uint64 newTimeout
    ) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(newTimeout > 0, "Decision timeout must be greater than 0");
        uint64 oldTimeout = DECISION_TIMEOUT;
        DECISION_TIMEOUT = newTimeout;

        // Emit an event for the timeout change
        emit DecisionTimeoutUpdated(oldTimeout, newTimeout);
    }

    /// @notice Updates the minimum collateral increase
    /// @param newMinIncrease The new minimum collateral increase
    /// @dev Can only be called by accounts with DEFAULT_ADMIN_ROLE
    function updateMinCollateralIncrease(
        uint256 newMinIncrease
    ) external onlyRole(DEFAULT_ADMIN_ROLE) {
        require(
            newMinIncrease > 0,
            "Min collateral increase must be greater than 0"
        );
        uint256 oldMinIncrease = MIN_COLLATERAL_INCREASE;
        MIN_COLLATERAL_INCREASE = newMinIncrease;

        // Emit an event for the min increase change
        emit MinCollateralIncreaseUpdated(oldMinIncrease, newMinIncrease);
    }

    /// @dev Function to authorize upgrades, restricted to UPGRADER_ROLE
    function _authorizeUpgrade(
        address newImplementation
    ) internal override onlyRole(UPGRADER_ROLE) {
        emit ContractUpgraded(this.getVersion() + 1, newImplementation);
    }

    // Additional events for administrative changes
    event TrusteeUpdated(
        address indexed oldTrustee,
        address indexed newTrustee
    );
    event DecisionTimeoutUpdated(uint64 oldTimeout, uint64 newTimeout);
    event MinCollateralIncreaseUpdated(
        uint256 oldMinIncrease,
        uint256 newMinIncrease
    );
}
