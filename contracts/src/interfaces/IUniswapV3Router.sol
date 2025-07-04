// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

/**
 * @title IUniswapV3Router
 * @dev Interface for Uniswap V3 Router (SwapRouter)
 */
interface IUniswapV3Router {
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    /**
     * @notice Swaps `amountIn` of one token for as much as possible of another token
     * @param params The parameters necessary for the swap, encoded as `ExactInputSingleParams`
     * @return amountOut The amount of the received token
     */
    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut);

    struct ExactInputParams {
        bytes path;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
    }

    /**
     * @notice Swaps `amountIn` of one token for as much as possible of another along the specified path
     * @param params The parameters necessary for the multi-hop swap, encoded as `ExactInputParams`
     * @return amountOut The amount of the received token
     */
    function exactInput(ExactInputParams calldata params) external payable returns (uint256 amountOut);

    struct ExactOutputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 deadline;
        uint256 amountOut;
        uint256 amountInMaximum;
        uint160 sqrtPriceLimitX96;
    }

    /**
     * @notice Swaps as little as possible of one token for `amountOut` of another token
     * @param params The parameters necessary for the swap, encoded as `ExactOutputSingleParams`
     * @return amountIn The amount of the input token
     */
    function exactOutputSingle(ExactOutputSingleParams calldata params) external payable returns (uint256 amountIn);

    struct ExactOutputParams {
        bytes path;
        address recipient;
        uint256 deadline;
        uint256 amountOut;
        uint256 amountInMaximum;
    }

    /**
     * @notice Swaps as little as possible of one token for `amountOut` of another along the specified path (reversed)
     * @param params The parameters necessary for the multi-hop swap, encoded as `ExactOutputParams`
     * @return amountIn The amount of the input token
     */
    function exactOutput(ExactOutputParams calldata params) external payable returns (uint256 amountIn);
}
