// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

/**
 * @title ICurvePool
 * @dev Interface for Curve Pool
 * Note: This is a simplified interface that covers the main functions used for arbitrage
 */
interface ICurvePool {
    /**
     * @dev Performs an exchange between two coins
     * @param i Index value for the coin to send
     * @param j Index value of the coin to receive
     * @param dx Amount of i being exchanged
     * @param min_dy Minimum amount of j to receive
     * @return Amount of j received
     */
    function exchange(
        int128 i,
        int128 j,
        uint256 dx,
        uint256 min_dy
    ) external returns (uint256);

    /**
     * @dev Performs an exchange between two coins with ETH
     * @param i Index value for the coin to send
     * @param j Index value of the coin to receive
     * @param dx Amount of i being exchanged
     * @param min_dy Minimum amount of j to receive
     * @return Amount of j received
     */
    function exchange_underlying(
        int128 i,
        int128 j,
        uint256 dx,
        uint256 min_dy
    ) external returns (uint256);

    /**
     * @dev Get the amount of coin j received for swapping dx of coin i
     * @param i Index value for the coin to send
     * @param j Index value of the coin to receive
     * @param dx Amount of i being exchanged
     * @return Amount of j that would be received
     */
    function get_dy(
        int128 i,
        int128 j,
        uint256 dx
    ) external view returns (uint256);

    /**
     * @dev Get the amount of coin j received for swapping dx of coin i, using underlying coins
     * @param i Index value for the coin to send
     * @param j Index value of the coin to receive
     * @param dx Amount of i being exchanged
     * @return Amount of j that would be received
     */
    function get_dy_underlying(
        int128 i,
        int128 j,
        uint256 dx
    ) external view returns (uint256);

    /**
     * @dev Get the virtual price of the pool LP token
     * @return The virtual price, scaled to the 18th decimal
     */
    function get_virtual_price() external view returns (uint256);

    /**
     * @dev Get the amount of coins in the pool
     * @return The number of coins in the pool
     */
    function coins(uint256 i) external view returns (address);

    /**
     * @dev Get the underlying coins in the pool (for metapools)
     * @return The address of the underlying coin
     */
    function underlying_coins(uint256 i) external view returns (address);

    /**
     * @dev Get the balance of a coin in the pool
     * @param i Index of the coin
     * @return The balance of the coin
     */
    function balances(uint256 i) external view returns (uint256);

    /**
     * @dev Get the A parameter of the pool
     * @return The A parameter
     */
    function A() external view returns (uint256);

    /**
     * @dev Get the fee of the pool
     * @return The fee
     */
    function fee() external view returns (uint256);
}
