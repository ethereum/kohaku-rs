use alloy::sol;

sol!(
    /// `TacitPaymaster.PaymasterData`: what `paymasterAndData` carries after the paymaster's
    /// address and its two gas limits.
    #[derive(Debug)]
    struct TacitPaymasterData {
        address pool;
        uint256[2] pA;
        uint256[2][2] pB;
        uint256[2] pC;
        uint256[11] publicInputs;
        address recipient;
        int256 extAmount;
        uint256 fee;
    }
);
