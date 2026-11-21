use alloy::sol;

sol! {
    interface EntryPoint {
        function getNonce(address sender, uint192 key) external view returns (uint256);
    }
}
