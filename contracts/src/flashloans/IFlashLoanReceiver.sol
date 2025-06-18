// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

/**
 * @title IFlashLoanReceiver
 * @dev Interface for flash loan receivers
 */
interface IFlashLoanReceiver {
    /**
     * @dev Callback function for Aave flash loans
     * @param assets The addresses of the assets being flash-borrowed
     * @param amounts The amounts of the assets being flash-borrowed
     * @param premiums The fee to be paid for each asset
     * @param initiator The address initiating the flash loan
     * @param params Arbitrary bytes-encoded params
     * @return Boolean indicating if the execution of the operation succeeded
     */
    function executeOperation(
        address[] calldata assets,
        uint256[] calldata amounts,
        uint256[] calldata premiums,
        address initiator,
        bytes calldata params
    ) external returns (bool);

    /**
     * @dev Callback function for Balancer flash loans
     * @param tokens The addresses of the tokens being flash-borrowed
     * @param amounts The amounts of the tokens being flash-borrowed
     * @param feeAmounts The fee to be paid for each token
     * @param userData Arbitrary bytes-encoded params
     */
    function receiveFlashLoan(
        address[] memory tokens,
        uint256[] memory amounts,
        uint256[] memory feeAmounts,
        bytes memory userData
    ) external;
}
