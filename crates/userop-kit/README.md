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
#     UserOperationBuilder,
#     smart_account::SimpleAccount,
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

## Railgun withdrawal execution accounts

`SimpleAccount` supports an external eth-infinitism SimpleAccount v0.8.0
address, including counterfactual deployment through its factory. Its account
address is the UserOperation sender and withdrawal recipient; the EOA owner
signs the operation. `Simple7702Account` supports EIP-7702 delegation to
the implementation passed to `new`, such as
`SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08`, which is not deployed on every chain.
The implementation must preserve the v0.8 EntryPoint EIP-712 validation and
atomic `executeBatch(Call[])` behavior.

For an external owner signer, send `operation.typed_data()` to
`eth_signTypedData_v4`, then attach the returned signature with
`operation.into_signed(owner, signature)?`. An EIP-7702 authorization must
already be signed. `sign(&owner)` remains available for local signers.

The `bundler` module adds ERC-4337 bundler methods to any alloy provider
connected to a bundler, such as `RootProvider::new_http(bundler_url)`:
`BundlerApi` for the standard methods, and `PimlicoApi` for Pimlico's gas
price. `send_user_operation` returns a `PendingUserOperation`; call
`.get_receipt()` on it to wait for inclusion.

Use final bundler simulation before broadcasting, including factory gas and
the full withdrawal call sequence. Keep the account recoverable if execution
fails after a validation-phase payout or postOp refund. Account bytecode and
factories are external to this crate. Reference source:
[eth-infinitism v0.8.0](https://github.com/eth-infinitism/account-abstraction/tree/v0.8.0/contracts/accounts),
commit `4cbc06072cdc19fd60f285c5997f4f7f57a588de`.
