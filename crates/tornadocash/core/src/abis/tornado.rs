use alloy::sol;

sol!(
    contract Tornado {
        event Deposit(bytes32 indexed commitment, uint32 leafIndex, uint256 timestamp);
        event Withdrawal(address to, bytes32 nullifierHash, address indexed relayer, uint256 fee);

        // @dev Deposit funds into the contract.
        function deposit(bytes32 _commitment) external payable;

        // @dev Withdraw a deposit from the contract.
        function withdraw(bytes calldata _proof, bytes32 _root, bytes32 _nullifierHash, address payable _recipient, address payable _relayer, uint256 _fee, uint256 _refund) external payable;
    }
);
