# userop-kit

ERC-4337 user operation builder and bundler client (EntryPoint 0.7 & 0.8).

## Execution accounts

`Simple7702SmartAccount::new(provider, owner, chain_id)` uses the v0.8 EntryPoint
and the default Simple7702 implementation. `.with_implementation(address)` selects
a compatible external target. The target must support v0.8 EntryPoint EIP-712
ECDSA signatures and the atomic `executeBatch(Call[])` ABI.

`smart_account::simple_account::SimpleAccount` supports eth-infinitism
SimpleAccount v0.8.0, whether already deployed or created through its factory:

```rust,no_run
# use alloy::{providers::DynProvider, primitives::{Address, U256}};
# use kohaku_userop_kit::{builder::UserOperationBuilder, smart_account::simple_account::SimpleAccount};
# async fn example(provider: DynProvider, factory: Address, owner: Address) -> anyhow::Result<()> {
let account = SimpleAccount::from_factory(provider, factory, owner, U256::ZERO, 1).await?;
let operation = UserOperationBuilder::new_with_smart_account(&account).await?.build();
let payload = operation.typed_data(); // send to the external owner's eth_signTypedData_v4
// let signed = operation.with_signature(owner, returned_signature)?;
# let _ = payload;
# Ok(())
# }
```

`SimpleAccount::new(provider, account_address, chain_id)` uses an existing account.
`from_factory` resolves its address with `getAddress(owner, salt)` and supplies
`createAccount(owner, salt)` only while that address has no code. The account's
EntryPoint nonce is independent of the owner's native transaction nonce.

`SmartAccount::init` returns one `AccountInit` variant: no initialization, EIP-7702
delegation, or factory deployment. Custom account implementations select that
variant and provide their signing domain, nonce and execution encoding. The
account-backed builder cannot combine delegation and factory initialization.

The smart account is the UserOperation sender; its EOA owner signs the operation.
`typed_data` returns the EIP-712 payload. `with_signature(owner, signature)` consumes
the signable operation, verifies recovery against `owner`, and attaches the
signature without copying the operation. Call `.clone()` first if another copy is
needed. A 7702 authorization must already be signed before using `with_signature`;
`sign(&owner)` remains available for local signing of both operation and authorization.

Account implementations and factory deployments are external configuration. No
Solidity or bytecode is bundled. Reference source: [eth-infinitism v0.8.0](https://github.com/eth-infinitism/account-abstraction/tree/v0.8.0/contracts/accounts),
commit `4cbc06072cdc19fd60f285c5997f4f7f57a588de`.

## Privacy-specific account reuse checks

The generic builder accepts both fresh and previously used accounts. It reads the
current EntryPoint nonce and, for delegation, the current native authorization
nonce. It does not impose an account freshness policy.

Privacy applications can use the separate `kohaku-account-checks` crate before
proving or signing. Its `is_address_unused` and `is_sender_unused` functions check
current address state and the default EntryPoint nonce independently. Apply these
checks unless the application caller explicitly opts into reuse, for example with
`allow_used: true`. RPC and decoding failures return errors. These checks do not
enumerate ERC-20 balances, other nonce keys, or activity that leaves no current
state signal.
