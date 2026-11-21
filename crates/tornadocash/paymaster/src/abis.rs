use alloy::sol;

sol!(
    #[sol(rpc)]
    contract TornadoPaymaster {
        function quoteWeiInToken(address feeToken, uint256 weiAmount) external view returns (uint256 tokenAmount);
    }

    struct TornadoPaymasterData {
        address instance;
        bytes proof;
        bytes32 root;
        bytes32 nullifierHash;
        address recipient;
        uint256 fee;
    }
);
