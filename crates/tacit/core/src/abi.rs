//! The pool's interface. `transact` and its event take more arguments than clippy likes.
#![allow(clippy::too_many_arguments)]

use alloy::sol;

sol! {
    /// The Tacit pool: one immutable contract per chain, native ETH.
    #[sol(rpc)]
    #[derive(Debug)]
    interface TacitPool {
        function transact(
            uint256[2] pA,
            uint256[2][2] pB,
            uint256[2] pC,
            uint256[11] publicInputs,
            address recipient,
            int256 extAmount,
            address relayer,
            uint256 fee,
            bytes memo0,
            bytes memo1
        ) external payable;

        /// How many leaves the tree had when `root` was its head. Reverts for a root it never held.
        function rootSize(bytes32 root) external view returns (uint256);

        /// The current root and leaf count.
        function head() external view returns (bytes32 root, uint256 size);

        event Transact(
            bytes32 indexed nf0,
            bytes32 indexed nf1,
            bytes32 outLeaf0,
            bytes32 outLeaf1,
            uint256 firstIndex,
            bytes32 newRoot,
            address recipient,
            int256 extAmount,
            address relayer,
            uint256 fee,
            bytes memo0,
            bytes memo1
        );
    }
}
