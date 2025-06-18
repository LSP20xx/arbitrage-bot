// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

/**
 * @title IBalancerFlashLoanProvider
 * @dev Interface for Balancer flash loan provider (Vault)
 */
interface IBalancerFlashLoanProvider {
    /**
     * @dev Performs a flash loan. The caller must implement the IFlashLoanReceiver interface.
     * @param recipient The address of the flash loan receiver
     * @param tokens The addresses of the tokens to be flash loaned
     * @param amounts The amounts of the tokens to be flash loaned
     * @param userData User-defined data to be passed to the receiver
     */
    function flashLoan(
        address recipient,
        address[] memory tokens,
        uint256[] memory amounts,
        bytes memory userData
    ) external;

    /**
     * @dev Returns the fee on flash loans
     * @return The fee on flash loans, expressed as a percentage of the loan amount
     */
    function getFlashLoanFeePercentage() external view returns (uint256);
}
