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

## Railgun withdrawal execution accounts

`DeployedSimpleAccount` supports an external eth-infinitism SimpleAccount v0.8.0
address, including counterfactual deployment through its factory. Its account
address is the UserOperation sender and withdrawal recipient; the EOA owner
signs the operation. `Simple7702SmartAccount` and `SimpleSmartAccount` support
EIP-7702 delegation to the default Simple7702 implementation or a compatible
external implementation selected with `.with_implementation(address)?`. The
selected implementation must preserve the v0.8 EntryPoint EIP-712 validation
and atomic `executeBatch(Call[])` behavior.

These Railgun account adapters check code, native and EntryPoint nonces, and
ETH balance before building an operation. A previously used sender requires an
explicit `.allow_used(true)`. RPC failures stop the preflight. ERC20 holdings
and past activity without a current-state signal are outside this check.
Custom `SmartAccount` adapters can implement `validate_sender` for their own
reuse policy.

For an external owner signer, send `operation.typed_data()` to
`eth_signTypedData_v4`, then attach the returned signature with
`operation.with_owner_signature(owner, signature)?` or
`operation.with_signature(owner, signature)?`. The former preserves the
signable operation. An EIP-7702 authorization must already be signed for
either method. `sign(&owner)` remains available for local signers.

Use final bundler simulation before broadcasting, including factory gas and
the full withdrawal call sequence. Keep the account recoverable if execution
fails after a validation-phase payout or postOp refund. Account bytecode and
factories are external to this crate. Reference source:
[eth-infinitism v0.8.0](https://github.com/eth-infinitism/account-abstraction/tree/v0.8.0/contracts/accounts),
commit `4cbc06072cdc19fd60f285c5997f4f7f57a588de`.
