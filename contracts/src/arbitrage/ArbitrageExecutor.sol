// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

import "../interfaces/IERC20.sol";
import "../interfaces/IWETH.sol";
import "../interfaces/IUniswapV2Router02.sol";
import "../interfaces/IUniswapV3Router.sol";
import "../interfaces/ICurvePool.sol";
import "../libraries/SafeERC20.sol";
import "../flashloans/IFlashLoanReceiver.sol";
import "../flashloans/IAaveFlashLoanProvider.sol";
import "../flashloans/IBalancerFlashLoanProvider.sol";

/**
 * @title ArbitrageExecutor
 * @dev Contract for executing arbitrage opportunities across different DEXes
 * Supports flash loans from various providers and multiple DEX interactions
 */
contract ArbitrageExecutor is IFlashLoanReceiver {
    using SafeERC20 for IERC20;

    // Constants
    address private constant WETH = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2; // Mainnet WETH
    uint256 private constant MAX_UINT = type(uint256).max;

    // State variables
    address public owner;
    bool public paused;
    mapping(address => bool) public operators;
    mapping(address => bool) public approvedDexes;
    mapping(address => bool) public approvedFlashLoanProviders;

    // Structs
    struct SwapStep {
        address dex;
        address tokenIn;
        address tokenOut;
        uint256 amountIn;
        uint256 minAmountOut;
        bytes data;
    }

    struct ArbitrageParams {
        address flashLoanProvider;
        address flashLoanToken;
        uint256 flashLoanAmount;
        SwapStep[] swapSteps;
        uint256 expectedProfit;
        uint256 deadline;
    }

    // Events
    event ArbitrageExecuted(
        address indexed executor,
        address indexed flashLoanToken,
        uint256 flashLoanAmount,
        uint256 profit
    );
    event OperatorAdded(address indexed operator);
    event OperatorRemoved(address indexed operator);
    event DexApproved(address indexed dex);
    event DexRemoved(address indexed dex);
    event FlashLoanProviderApproved(address indexed provider);
    event FlashLoanProviderRemoved(address indexed provider);
    event Paused(address indexed account);
    event Unpaused(address indexed account);
    event FundsRescued(address indexed token, address indexed to, uint256 amount);

    // Modifiers
    modifier onlyOwner() {
        require(msg.sender == owner, "ArbitrageExecutor: caller is not the owner");
        _;
    }

    modifier onlyOperator() {
        require(
            msg.sender == owner || operators[msg.sender],
            "ArbitrageExecutor: caller is not an operator"
        );
        _;
    }

    modifier whenNotPaused() {
        require(!paused, "ArbitrageExecutor: contract is paused");
        _;
    }

    /**
     * @dev Constructor
     */
    constructor() {
        owner = msg.sender;
        operators[msg.sender] = true;
    }

    /**
     * @dev Receive function to accept ETH
     */
    receive() external payable {}

    /**
     * @dev Execute an arbitrage opportunity
     * @param params The arbitrage parameters
     */
    function executeArbitrage(ArbitrageParams calldata params)
        external
        onlyOperator
        whenNotPaused
    {
        require(
            params.deadline >= block.timestamp,
            "ArbitrageExecutor: deadline expired"
        );
        require(
            params.swapSteps.length > 0,
            "ArbitrageExecutor: no swap steps provided"
        );
        require(
            approvedFlashLoanProviders[params.flashLoanProvider],
            "ArbitrageExecutor: flash loan provider not approved"
        );

        // Validate all DEXes in the swap steps
        for (uint256 i = 0; i < params.swapSteps.length; i++) {
            require(
                approvedDexes[params.swapSteps[i].dex],
                "ArbitrageExecutor: dex not approved"
            );
        }

        // Execute flash loan
        if (params.flashLoanProvider == address(0)) {
            // No flash loan, use contract's own funds
            executeSwaps(params.swapSteps, params.expectedProfit);
        } else if (_isAaveProvider(params.flashLoanProvider)) {
            // Aave flash loan
            address[] memory assets = new address[](1);
            assets[0] = params.flashLoanToken;

            uint256[] memory amounts = new uint256[](1);
            amounts[0] = params.flashLoanAmount;

            uint256[] memory modes = new uint256[](1);
            modes[0] = 0; // no debt - pay all back

            bytes memory data = abi.encode(params.swapSteps, params.expectedProfit);

            IAaveFlashLoanProvider(params.flashLoanProvider).flashLoan(
                address(this),
                assets,
                amounts,
                modes,
                address(this),
                data,
                0 // referral code
            );
        } else if (_isBalancerProvider(params.flashLoanProvider)) {
            // Balancer flash loan
            address[] memory tokens = new address[](1);
            tokens[0] = params.flashLoanToken;

            uint256[] memory amounts = new uint256[](1);
            amounts[0] = params.flashLoanAmount;

            bytes memory userData = abi.encode(params.swapSteps, params.expectedProfit);

            IBalancerFlashLoanProvider(params.flashLoanProvider).flashLoan(
                address(this),
                tokens,
                amounts,
                userData
            );
        } else {
            revert("ArbitrageExecutor: unsupported flash loan provider");
        }
    }

    /**
     * @dev Callback function for Aave flash loans
     */
    function executeOperation(
        address[] calldata assets,
        uint256[] calldata amounts,
        uint256[] calldata premiums,
        address initiator,
        bytes calldata params
    ) external override returns (bool) {
        require(
            initiator == address(this),
            "ArbitrageExecutor: initiator must be this contract"
        );
        require(
            approvedFlashLoanProviders[msg.sender],
            "ArbitrageExecutor: flash loan provider not approved"
        );

        // Decode parameters
        (SwapStep[] memory swapSteps, uint256 expectedProfit) = abi.decode(
            params,
            (SwapStep[], uint256)
        );

        // Execute swaps
        executeSwaps(swapSteps, expectedProfit);

        // Approve flash loan repayment
        uint256 totalRepayment = amounts[0] + premiums[0];
        IERC20(assets[0]).safeApprove(msg.sender, totalRepayment);

        return true;
    }

    /**
     * @dev Callback function for Balancer flash loans
     */
    function receiveFlashLoan(
        address[] memory tokens,
        uint256[] memory amounts,
        uint256[] memory feeAmounts,
        bytes memory userData
    ) external override {
        require(
            approvedFlashLoanProviders[msg.sender],
            "ArbitrageExecutor: flash loan provider not approved"
        );

        // Decode parameters
        (SwapStep[] memory swapSteps, uint256 expectedProfit) = abi.decode(
            userData,
            (SwapStep[], uint256)
        );

        // Execute swaps
        executeSwaps(swapSteps, expectedProfit);

        // Repay flash loan
        for (uint256 i = 0; i < tokens.length; i++) {
            uint256 totalRepayment = amounts[i] + feeAmounts[i];
            IERC20(tokens[i]).safeTransfer(msg.sender, totalRepayment);
        }
    }

    /**
     * @dev Execute a series of swaps
     * @param swapSteps The swap steps to execute
     * @param expectedProfit The expected profit
     */
    function executeSwaps(
        SwapStep[] memory swapSteps,
        uint256 expectedProfit
    ) internal {
        require(swapSteps.length > 0, "ArbitrageExecutor: no swap steps provided");

        // Track initial and final balances to verify profit
        address initialToken = swapSteps[0].tokenIn;
        address finalToken = swapSteps[swapSteps.length - 1].tokenOut;

        uint256 initialBalance = IERC20(initialToken).balanceOf(address(this));

        // Execute each swap step
        for (uint256 i = 0; i < swapSteps.length; i++) {
            SwapStep memory step = swapSteps[i];

            // Approve token spending
            IERC20(step.tokenIn).safeApprove(step.dex, step.amountIn);

            // Execute the swap based on DEX type
            if (_isUniswapV2Router(step.dex)) {
                _executeUniswapV2Swap(step);
            } else if (_isUniswapV3Router(step.dex)) {
                _executeUniswapV3Swap(step);
            } else if (_isCurvePool(step.dex)) {
                _executeCurveSwap(step);
            } else {
                revert("ArbitrageExecutor: unsupported DEX");
            }

            // Reset approval
            IERC20(step.tokenIn).safeApprove(step.dex, 0);
        }

        // Verify profit
        uint256 finalBalance = IERC20(finalToken).balanceOf(address(this));
        uint256 actualProfit = finalBalance - initialBalance;

        require(
            actualProfit >= expectedProfit,
            "ArbitrageExecutor: profit less than expected"
        );

        emit ArbitrageExecuted(
            msg.sender,
            initialToken,
            initialBalance,
            actualProfit
        );
    }

    /**
     * @dev Execute a Uniswap V2 swap
     * @param step The swap step
     */
    function _executeUniswapV2Swap(SwapStep memory step) internal {
        IUniswapV2Router02 router = IUniswapV2Router02(step.dex);

        address[] memory path = new address[](2);
        path[0] = step.tokenIn;
        path[1] = step.tokenOut;

        router.swapExactTokensForTokens(
            step.amountIn,
            step.minAmountOut,
            path,
            address(this),
            block.timestamp
        );
    }

    /**
     * @dev Execute a Uniswap V3 swap
     * @param step The swap step
     */
    function _executeUniswapV3Swap(SwapStep memory step) internal {
        IUniswapV3Router router = IUniswapV3Router(step.dex);

        IUniswapV3Router.ExactInputSingleParams memory params =
            IUniswapV3Router.ExactInputSingleParams({
                tokenIn: step.tokenIn,
                tokenOut: step.tokenOut,
                fee: 3000, // Default to 0.3% fee tier, can be overridden in data
                recipient: address(this),
                deadline: block.timestamp,
                amountIn: step.amountIn,
                amountOutMinimum: step.minAmountOut,
                sqrtPriceLimitX96: 0
            });

        // If custom data is provided, use it to override the default params
        if (step.data.length > 0) {
            params = abi.decode(step.data, (IUniswapV3Router.ExactInputSingleParams));
        }

        router.exactInputSingle(params);
    }

    /**
     * @dev Execute a Curve swap
     * @param step The swap step
     */
    function _executeCurveSwap(SwapStep memory step) internal {
        ICurvePool pool = ICurvePool(step.dex);

        // For Curve, we need to know the token indices
        // This can be provided in the data field
        (int128 i, int128 j) = abi.decode(step.data, (int128, int128));

        pool.exchange(i, j, step.amountIn, step.minAmountOut);
    }

    /**
     * @dev Check if a provider is an Aave flash loan provider
     * @param provider The provider address
     * @return True if the provider is an Aave flash loan provider
     */
    function _isAaveProvider(address provider) internal pure returns (bool) {
        // In a real implementation, we would check the provider's interface
        // For simplicity, we'll just return true here
        return true;
    }

    /**
     * @dev Check if a provider is a Balancer flash loan provider
     * @param provider The provider address
     * @return True if the provider is a Balancer flash loan provider
     */
    function _isBalancerProvider(address provider) internal pure returns (bool) {
        // In a real implementation, we would check the provider's interface
        // For simplicity, we'll just return true here
        return true;
    }

    /**
     * @dev Check if a DEX is a Uniswap V2 router
     * @param dex The DEX address
     * @return True if the DEX is a Uniswap V2 router
     */
    function _isUniswapV2Router(address dex) internal pure returns (bool) {
        // In a real implementation, we would check the DEX's interface
        // For simplicity, we'll just return true here
        return true;
    }

    /**
     * @dev Check if a DEX is a Uniswap V3 router
     * @param dex The DEX address
     * @return True if the DEX is a Uniswap V3 router
     */
    function _isUniswapV3Router(address dex) internal pure returns (bool) {
        // In a real implementation, we would check the DEX's interface
        // For simplicity, we'll just return true here
        return true;
    }

    /**
     * @dev Check if a DEX is a Curve pool
     * @param dex The DEX address
     * @return True if the DEX is a Curve pool
     */
    function _isCurvePool(address dex) internal pure returns (bool) {
        // In a real implementation, we would check the DEX's interface
        // For simplicity, we'll just return true here
        return true;
    }

    /**
     * @dev Add an operator
     * @param _operator The operator address
     */
    function addOperator(address _operator) external onlyOwner {
        require(_operator != address(0), "ArbitrageExecutor: invalid operator");
        operators[_operator] = true;
        emit OperatorAdded(_operator);
    }

    /**
     * @dev Remove an operator
     * @param _operator The operator address
     */
    function removeOperator(address _operator) external onlyOwner {
        operators[_operator] = false;
        emit OperatorRemoved(_operator);
    }

    /**
     * @dev Approve a DEX
     * @param _dex The DEX address
     */
    function approveDex(address _dex) external onlyOwner {
        require(_dex != address(0), "ArbitrageExecutor: invalid DEX");
        approvedDexes[_dex] = true;
        emit DexApproved(_dex);
    }

    /**
     * @dev Remove a DEX
     * @param _dex The DEX address
     */
    function removeDex(address _dex) external onlyOwner {
        approvedDexes[_dex] = false;
        emit DexRemoved(_dex);
    }

    /**
     * @dev Approve a flash loan provider
     * @param _provider The provider address
     */
    function approveFlashLoanProvider(address _provider) external onlyOwner {
        require(_provider != address(0), "ArbitrageExecutor: invalid provider");
        approvedFlashLoanProviders[_provider] = true;
        emit FlashLoanProviderApproved(_provider);
    }

    /**
     * @dev Remove a flash loan provider
     * @param _provider The provider address
     */
    function removeFlashLoanProvider(address _provider) external onlyOwner {
        approvedFlashLoanProviders[_provider] = false;
        emit FlashLoanProviderRemoved(_provider);
    }

    /**
     * @dev Pause the contract
     */
    function pause() external onlyOwner {
        paused = true;
        emit Paused(msg.sender);
    }

    /**
     * @dev Unpause the contract
     */
    function unpause() external onlyOwner {
        paused = false;
        emit Unpaused(msg.sender);
    }

    /**
     * @dev Rescue tokens stuck in the contract
     * @param _token The token address (use address(0) for ETH)
     * @param _to The recipient address
     * @param _amount The amount to rescue
     */
    function rescueFunds(
        address _token,
        address _to,
        uint256 _amount
    ) external onlyOwner {
        require(_to != address(0), "ArbitrageExecutor: invalid recipient");

        if (_token == address(0)) {
            // Rescue ETH
            (bool success, ) = _to.call{value: _amount}("");
            require(success, "ArbitrageExecutor: ETH transfer failed");
        } else {
            // Rescue ERC20
            IERC20(_token).safeTransfer(_to, _amount);
        }

        emit FundsRescued(_token, _to, _amount);
    }

    /**
     * @dev Wrap ETH to WETH
     * @param _amount The amount to wrap
     */
    function wrapETH(uint256 _amount) external onlyOperator {
        IWETH(WETH).deposit{value: _amount}();
    }

    /**
     * @dev Unwrap WETH to ETH
     * @param _amount The amount to unwrap
     */
    function unwrapETH(uint256 _amount) external onlyOperator {
        IWETH(WETH).withdraw(_amount);
    }

    /**
     * @dev Transfer ownership of the contract
     * @param _newOwner The new owner address
     */
    function transferOwnership(address _newOwner) external onlyOwner {
        require(_newOwner != address(0), "ArbitrageExecutor: invalid owner");
        owner = _newOwner;
    }
}
