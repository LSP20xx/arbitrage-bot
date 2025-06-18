// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

/**
 * @title IAaveFlashLoanProvider
 * @dev Interface for Aave flash loan provider
 */
interface IAaveFlashLoanProvider {
    /**
     * @dev Allows a user to take a flash loan
     * @param receiverAddress The address of the contract receiving the funds
     * @param assets The addresses of the assets being flash-borrowed
     * @param amounts The amounts of the assets being flash-borrowed
     * @param modes The modes of the debt to be opened (0 = no debt, 1 = stable, 2 = variable)
     * @param onBehalfOf The address that will receive the debt in case of mode = 1 or mode = 2
     * @param params Arbitrary bytes-encoded params to pass to the receiver
     * @param referralCode Code used to register the integrator originating the operation
     */
    function flashLoan(
        address receiverAddress,
        address[] calldata assets,
        uint256[] calldata amounts,
        uint256[] calldata modes,
        address onBehalfOf,
        bytes calldata params,
        uint16 referralCode
    ) external;

    /**
     * @dev Returns the fee on flash loans
     * @return The fee on flash loans
     */
    function FLASHLOAN_PREMIUM_TOTAL() external view returns (uint256);
}
