//! ABI of the SimpleAccount and Simple7702Account batch execution.

use alloy::sol;

sol! {
    contract BaseAccount {
        struct Call {
            address target;
            uint256 value;
            bytes data;
        }

        function executeBatch(Call[] calldata calls) external;
    }
}
