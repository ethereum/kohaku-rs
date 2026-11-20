//! ABI of the SimpleAccount v0.8 factory.

use alloy::sol;

sol! {
    interface SimpleAccountFactory {
        function getAddress(address owner, uint256 salt) external view returns (address);
        function createAccount(address owner, uint256 salt) external returns (address);
    }
}
