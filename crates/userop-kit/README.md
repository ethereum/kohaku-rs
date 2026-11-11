# userop-kit

ERC-4337 user operation builder and bundler client (EntryPoint 0.7 & 0.8).

Supports EIP-7702 delegation, existing smart accounts, and factory deployment.

## Example

```rust,no_run
# use alloy::{
#     primitives::{Address, U256},
#     providers::DynProvider,
# };
# use kohaku_userop_kit::{
#     builder::UserOperationBuilder,
#     smart_account::simple_account::SimpleAccount,
# };
#
# async fn example(
#     provider: DynProvider,
#     factory: Address,
#     owner: Address,
# ) -> anyhow::Result<()> {
let account = SimpleAccount::from_factory(provider, factory, owner, U256::ZERO, 1).await?;
let operation = UserOperationBuilder::new_with_smart_account(&account)
    .await?
    .build();

// Send this payload to the owner's eth_signTypedData_v4 signer.
let payload = operation.typed_data();

# let _ = payload;
# Ok(())
# }
```
