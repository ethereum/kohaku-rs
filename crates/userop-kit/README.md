# userop-kit

ERC-4337 user operation builder and bundler client (EntryPoint 0.7 & 0.8).


## Withdrawal execution accounts

`Simple7702SmartAccount::new(provider, owner, chain_id)` keeps its existing default.
Use `.with_implementation(address)?` for a compatible, externally deployed 7702
implementation. The target must preserve the v0.8 EntryPoint EIP-712 ECDSA signature
and atomic `executeBatch(Call[])` behavior. This is an explicit integrator trust
choice; selecting a target does not certify its code.

Use `smart_account::deployed_simple_account::DeployedSimpleAccount` for the external
eth-infinitism **SimpleAccount v0.8.0** account format:

```rust,no_run
# use alloy::{providers::DynProvider, primitives::{Address, U256}};
# use kohaku_userop_kit::{builder::UserOperationBuilder, smart_account::deployed_simple_account::DeployedSimpleAccount};
# async fn example(provider: DynProvider, factory: Address, owner: Address) -> anyhow::Result<()> {
let account = DeployedSimpleAccount::from_factory(provider, factory, owner, U256::ZERO, 1).await?;
let operation = UserOperationBuilder::new_with_smart_account(&account).await?.build();
let payload = operation.typed_data(); // send to the external owner's eth_signTypedData_v4
// let signed = operation.with_owner_signature(owner, returned_signature)?;
# let _ = payload;
# Ok(())
# }
```

`new(provider, account_address, chain_id)` uses an already deployed account.
`from_factory` resolves the counterfactual address using the external v0.8
SimpleAccountFactory and includes deployment only while the address has no code.
The account's EntryPoint nonce is read independently of the EOA owner's nonce.
The deployed route emits no `eip7702Auth` and requests only the owner UserOp
signature. `with_owner_signature` verifies recovery against the selected owner;
`sign(&owner)` also remains available for local signers. A delegation operation
must already have a signed authorization before using `with_owner_signature`.

The account address is the UserOp sender and the withdrawal recipient when its
execution spends the payout. The EOA owner is only its signer. Paymaster contracts,
proof formats, and protocol fee logic are unchanged. Use successful final bundler
simulation before broadcasting, including factory deployment gas and the complete
withdrawal call sequence. Keep the account recoverable if execution fails after a
validation-phase payout or a postOp refund.

No Solidity or account bytecode is vendored by this feature. Implementation and
factory deployments remain external configuration. Reference source:
[eth-infinitism v0.8.0](https://github.com/eth-infinitism/account-abstraction/tree/v0.8.0/contracts/accounts),
commit `4cbc06072cdc19fd60f285c5997f4f7f57a588de`. Upstream audit reports cover their
stated revisions and scope, not arbitrary custom targets or this SDK integration.


Built-in accounts reject touched senders by default, before building an operation:
code/delegation, a native or key-0 EntryPoint nonce, or an ETH balance require an
explicit `.allow_used(true)`. Use this for already deployed smart accounts and
previously delegated EOAs. The builder reads the account's current EntryPoint nonce
and delegation signing reads its current native nonce; factory deployment is omitted
once code exists. Checks fail closed on RPC errors. ERC20 holdings and history with
no current-state signal are not enumerated. Custom `SmartAccount` adapters must
implement `validate_sender` with the same policy if they allow existing senders.
