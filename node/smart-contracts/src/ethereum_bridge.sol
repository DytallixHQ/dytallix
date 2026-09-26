// SPDX-License-Identifier: MIT
pragma solidity ^0.8.19;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/security/ReentrancyGuard.sol";
import "@openzeppelin/contracts/security/Pausable.sol";

/**
 * @title DytallixBridge
 * @dev Cross-chain bridge contract for Ethereum (Sepolia) side
 * Handles token locking/unlocking mechanism with AI-enhanced security
 */
contract DytallixBridge is Ownable, ReentrancyGuard, Pausable {
    
    // Events for cross-chain communication
    event TokenLocked(
        address indexed token,
        address indexed user,
        uint256 amount,
        string cosmosAddress,
        bytes32 indexed bridgeId
    );
    
    event TokenUnlocked(
        address indexed token,
        address indexed user,
        uint256 amount,
        bytes32 indexed bridgeId
    );
    
    event BridgeInitiated(
        bytes32 indexed bridgeId,
        address indexed token,
        uint256 amount,
        address indexed user
    );
    
    event BridgeCompleted(
        bytes32 indexed bridgeId,
        bool success
    );
    
    event BridgeError(
        bytes32 indexed bridgeId,
        string reason
    );
    
    event ValidatorAdded(address indexed validator);
    event ValidatorRemoved(address indexed validator);
    event MinValidatorsChanged(uint256 oldMin, uint256 newMin);
    
    // Structs
    struct BridgeTransaction {
        address token;
        address user;
        uint256 amount;
        string cosmosAddress;
        uint256 timestamp;
        BridgeStatus status;
        uint256 validatorSignatures;
    }
    
    enum BridgeStatus {
        Pending,
        Locked,
        Completed,
        Failed,
        Expired
    }
    
    // State variables
    mapping(bytes32 => BridgeTransaction) public bridgeTransactions;
    mapping(address => bool) public validators;
    mapping(bytes32 => mapping(address => bool)) public validatorSignatures;
    mapping(address => uint256) public lockedBalances;
    mapping(address => bool) public supportedTokens;
    
    address[] public validatorList;
    uint256 public minValidators;
    uint256 public bridgeTimeout;
    uint256 public bridgeFee;
    uint256 public maxBridgeAmount;
    uint256 public minBridgeAmount;
    
    // Gas optimization settings
    uint256 public constant MAX_VALIDATORS = 100;
    uint256 public constant MIN_BRIDGE_AMOUNT_DEFAULT = 1e15; // 0.001 ETH equivalent
    uint256 public constant MAX_BRIDGE_AMOUNT_DEFAULT = 1e24; // 1M tokens (18 decimals)
    uint256 public constant BRIDGE_TIMEOUT_DEFAULT = 24 hours;
    
    // AI fraud detection integration
    address public aiOracle;
    uint256 public aiConfidenceThreshold;
    mapping(bytes32 => uint256) public aiRiskScores;
    
    modifier onlyValidator() {
        require(validators[msg.sender], "Not a validator");
        _;
    }
    
    modifier validBridgeId(bytes32 bridgeId) {
        require(bridgeTransactions[bridgeId].user != address(0), "Invalid bridge ID");
        _;
    }
    
    modifier supportedToken(address token) {
        require(supportedTokens[token], "Token not supported");
        _;
    }
    
    constructor(
        address[] memory _validators,
        uint256 _minValidators,
        address _aiOracle
    ) {
        require(_validators.length >= _minValidators, "Not enough validators");
        require(_minValidators > 0, "Min validators must be > 0");
        require(_aiOracle != address(0), "Invalid AI oracle address");
        
        for (uint256 i = 0; i < _validators.length; i++) {
            require(_validators[i] != address(0), "Invalid validator address");
            validators[_validators[i]] = true;
            validatorList.push(_validators[i]);
            emit ValidatorAdded(_validators[i]);
        }
        
        minValidators = _minValidators;
        bridgeTimeout = BRIDGE_TIMEOUT_DEFAULT;
        minBridgeAmount = MIN_BRIDGE_AMOUNT_DEFAULT;
        maxBridgeAmount = MAX_BRIDGE_AMOUNT_DEFAULT;
        aiOracle = _aiOracle;
        aiConfidenceThreshold = 80; // 80% confidence threshold
    }
    
    /**
     * @dev Lock tokens for cross-chain transfer
     * @param token Token contract address
     * @param amount Amount to lock
     * @param cosmosAddress Destination address on Cosmos
     */
    function lockTokens(
        address token,
        uint256 amount,
        string calldata cosmosAddress
    ) external payable nonReentrant whenNotPaused supportedToken(token) {
        require(amount >= minBridgeAmount, "Amount below minimum");
        require(amount <= maxBridgeAmount, "Amount above maximum");
        require(bytes(cosmosAddress).length > 0, "Invalid Cosmos address");
        require(msg.value >= bridgeFee, "Insufficient bridge fee");
        
        // Generate unique bridge ID
        bytes32 bridgeId = keccak256(
            abi.encodePacked(
                block.timestamp,
                block.difficulty,
                msg.sender,
                token,
                amount,
                cosmosAddress
            )
        );
        
        // Transfer tokens to contract
        IERC20(token).transferFrom(msg.sender, address(this), amount);
        
        // Update locked balance
        lockedBalances[token] += amount;
        
        // Create bridge transaction
        bridgeTransactions[bridgeId] = BridgeTransaction({
            token: token,
            user: msg.sender,
            amount: amount,
            cosmosAddress: cosmosAddress,
            timestamp: block.timestamp,
            status: BridgeStatus.Locked,
            validatorSignatures: 0
        });
        
        emit TokenLocked(token, msg.sender, amount, cosmosAddress, bridgeId);
        emit BridgeInitiated(bridgeId, token, amount, msg.sender);
        
        // Request AI fraud detection analysis
        _requestAIAnalysis(bridgeId);
    }
    
    /**
     * @dev Unlock tokens after cross-chain burn confirmation
     * @param bridgeId Bridge transaction ID
     * @param signature Validator signature for unlock authorization
     */
    function unlockTokens(
        bytes32 bridgeId,
        bytes calldata signature
    ) external onlyValidator validBridgeId(bridgeId) {
        BridgeTransaction storage bridgeTx = bridgeTransactions[bridgeId];
        require(bridgeTx.status == BridgeStatus.Locked, "Invalid status for unlock");
        require(!validatorSignatures[bridgeId][msg.sender], "Already signed");
        require(block.timestamp <= bridgeTx.timestamp + bridgeTimeout, "Bridge expired");
        
        // Verify signature (simplified - real implementation would verify cryptographic signature)
        require(signature.length > 0, "Invalid signature");
        
        // Check AI risk score
        require(aiRiskScores[bridgeId] < aiConfidenceThreshold, "AI detected high risk");
        
        // Record validator signature
        validatorSignatures[bridgeId][msg.sender] = true;
        bridgeTx.validatorSignatures++;
        
        // Check if enough signatures collected
        if (bridgeTx.validatorSignatures >= minValidators) {
            // Execute unlock
            _executeUnlock(bridgeId);
        }
    }
    
    /**
     * @dev Emergency unlock with multi-validator consensus
     * @param bridgeId Bridge transaction ID
     * @param reason Reason for emergency unlock
     */
    function emergencyUnlock(
        bytes32 bridgeId,
        string calldata reason
    ) external onlyValidator validBridgeId(bridgeId) {
        BridgeTransaction storage bridgeTx = bridgeTransactions[bridgeId];
        require(bridgeTx.status == BridgeStatus.Locked, "Invalid status");
        require(!validatorSignatures[bridgeId][msg.sender], "Already signed emergency");
        
        // Record emergency signature
        validatorSignatures[bridgeId][msg.sender] = true;
        bridgeTx.validatorSignatures++;
        
        // Require higher threshold for emergency unlock
        uint256 emergencyThreshold = (validatorList.length * 2) / 3; // 66.7%
        
        if (bridgeTx.validatorSignatures >= emergencyThreshold) {
            _executeUnlock(bridgeId);
            emit BridgeError(bridgeId, reason);
        }
    }
    
    /**
     * @dev Complete bridge transaction
     * @param bridgeId Bridge transaction ID
     * @param success Whether the bridge was successful
     */
    function completeBridge(
        bytes32 bridgeId,
        bool success
    ) external onlyValidator validBridgeId(bridgeId) {
        BridgeTransaction storage bridgeTx = bridgeTransactions[bridgeId];
        require(bridgeTx.status == BridgeStatus.Locked, "Invalid status");
        require(!validatorSignatures[bridgeId][msg.sender], "Already voted");
        
        validatorSignatures[bridgeId][msg.sender] = true;
        bridgeTx.validatorSignatures++;
        
        if (bridgeTx.validatorSignatures >= minValidators) {
            bridgeTx.status = success ? BridgeStatus.Completed : BridgeStatus.Failed;
            emit BridgeCompleted(bridgeId, success);
            
            if (!success) {
                // Refund locked tokens on failure
                _executeUnlock(bridgeId);
            }
        }
    }
    
    /**
     * @dev Add supported token
     * @param token Token contract address
     */
    function addSupportedToken(address token) external onlyOwner {
        require(token != address(0), "Invalid token address");
        supportedTokens[token] = true;
    }
    
    /**
     * @dev Remove supported token
     * @param token Token contract address
     */
    function removeSupportedToken(address token) external onlyOwner {
        supportedTokens[token] = false;
    }
    
    /**
     * @dev Add validator
     * @param validator Validator address
     */
    function addValidator(address validator) external onlyOwner {
        require(validator != address(0), "Invalid validator address");
        require(!validators[validator], "Already a validator");
        require(validatorList.length < MAX_VALIDATORS, "Too many validators");
        
        validators[validator] = true;
        validatorList.push(validator);
        emit ValidatorAdded(validator);
    }
    
    /**
     * @dev Remove validator
     * @param validator Validator address
     */
    function removeValidator(address validator) external onlyOwner {
        require(validators[validator], "Not a validator");
        require(validatorList.length > minValidators, "Cannot remove validator below minimum");
        
        validators[validator] = false;
        
        // Remove from validator list
        for (uint256 i = 0; i < validatorList.length; i++) {
            if (validatorList[i] == validator) {
                validatorList[i] = validatorList[validatorList.length - 1];
                validatorList.pop();
                break;
            }
        }
        
        emit ValidatorRemoved(validator);
    }
    
    /**
     * @dev Set minimum validators required
     * @param _minValidators New minimum validator count
     */
    function setMinValidators(uint256 _minValidators) external onlyOwner {
        require(_minValidators > 0, "Min validators must be > 0");
        require(_minValidators <= validatorList.length, "Min validators exceeds current count");
        
        uint256 oldMin = minValidators;
        minValidators = _minValidators;
        emit MinValidatorsChanged(oldMin, _minValidators);
    }
    
    /**
     * @dev Set bridge parameters
     */
    function setBridgeParameters(
        uint256 _bridgeFee,
        uint256 _bridgeTimeout,
        uint256 _minBridgeAmount,
        uint256 _maxBridgeAmount
    ) external onlyOwner {
        bridgeFee = _bridgeFee;
        bridgeTimeout = _bridgeTimeout;
        minBridgeAmount = _minBridgeAmount;
        maxBridgeAmount = _maxBridgeAmount;
    }
    
    /**
     * @dev Set AI oracle parameters
     */
    function setAIParameters(
        address _aiOracle,
        uint256 _confidenceThreshold
    ) external onlyOwner {
        require(_aiOracle != address(0), "Invalid AI oracle address");
        require(_confidenceThreshold <= 100, "Invalid confidence threshold");
        
        aiOracle = _aiOracle;
        aiConfidenceThreshold = _confidenceThreshold;
    }
    
    /**
     * @dev Update AI risk score for a bridge transaction
     * @param bridgeId Bridge transaction ID
     * @param riskScore Risk score from AI analysis (0-100)
     */
    function updateAIRiskScore(
        bytes32 bridgeId,
        uint256 riskScore
    ) external {
        require(msg.sender == aiOracle, "Only AI oracle can update risk scores");
        require(riskScore <= 100, "Invalid risk score");
        
        aiRiskScores[bridgeId] = riskScore;
    }
    
    /**
     * @dev Pause bridge operations
     */
    function pause() external onlyOwner {
        _pause();
    }
    
    /**
     * @dev Unpause bridge operations
     */
    function unpause() external onlyOwner {
        _unpause();
    }
    
    /**
     * @dev Withdraw bridge fees
     */
    function withdrawFees() external onlyOwner {
        uint256 balance = address(this).balance;
        require(balance > 0, "No fees to withdraw");
        
        payable(owner()).transfer(balance);
    }
    
    /**
     * @dev Emergency withdrawal of stuck tokens
     * @param token Token contract address
     * @param amount Amount to withdraw
     */
    function emergencyWithdraw(
        address token,
        uint256 amount
    ) external onlyOwner {
        require(amount <= IERC20(token).balanceOf(address(this)) - lockedBalances[token], 
                "Cannot withdraw locked tokens");
        
        IERC20(token).transfer(owner(), amount);
    }
    
    // View functions
    
    /**
     * @dev Get bridge transaction details
     */
    function getBridgeTransaction(bytes32 bridgeId) 
        external 
        view 
        returns (BridgeTransaction memory) 
    {
        return bridgeTransactions[bridgeId];
    }
    
    /**
     * @dev Get validator count
     */
    function getValidatorCount() external view returns (uint256) {
        return validatorList.length;
    }
    
    /**
     * @dev Get all validators
     */
    function getValidators() external view returns (address[] memory) {
        return validatorList;
    }
    
    /**
     * @dev Check if bridge transaction is expired
     */
    function isBridgeExpired(bytes32 bridgeId) external view returns (bool) {
        BridgeTransaction memory bridgeTx = bridgeTransactions[bridgeId];
        return block.timestamp > bridgeTx.timestamp + bridgeTimeout;
    }
    
    // Private functions
    
    /**
     * @dev Execute token unlock
     */
    function _executeUnlock(bytes32 bridgeId) private {
        BridgeTransaction storage bridgeTx = bridgeTransactions[bridgeId];
        
        // Update locked balance
        lockedBalances[bridgeTx.token] -= bridgeTx.amount;
        
        // Transfer tokens back to user
        IERC20(bridgeTx.token).transfer(bridgeTx.user, bridgeTx.amount);
        
        // Update status
        bridgeTx.status = BridgeStatus.Completed;
        
        emit TokenUnlocked(bridgeTx.token, bridgeTx.user, bridgeTx.amount, bridgeId);
    }
    
    /**
     * @dev Request AI analysis for fraud detection
     */
    function _requestAIAnalysis(bytes32 bridgeId) private {
        // This would integrate with the AI oracle for fraud detection
        // For now, we set a default low risk score
        aiRiskScores[bridgeId] = 10; // Low risk by default
    }
    
    /**
     * @dev Get bridge transaction hash for verification
     */
    function getBridgeHash(
        address token,
        address user,
        uint256 amount,
        string calldata cosmosAddress,
        uint256 timestamp
    ) external pure returns (bytes32) {
        return keccak256(abi.encodePacked(token, user, amount, cosmosAddress, timestamp));
    }
}