use alloy::sol;

sol!(
    #[sol(rpc)]
    contract Tornado {
        // @dev whether a note is already spent
        function isSpent(bytes32 _nullifierHash) public view returns(bool);

        // @dev whether an array of notes is already spent
        function isSpentArray(bytes32[] calldata _nullifierHashes) external view returns(bool[] memory spent);

        // @dev Whether the root is present in the root history
        function isKnownRoot(bytes32 _root) public view returns(bool);

        // @dev The contract that verifies withdrawal proofs
        function verifier() external view returns (address);
    }
);
