# tornadocash-paymaster

Client for interacting with Tornado Cash paymasters.

> [!IMPORTANT]
> The privacy-paymaster contracts are experimental and not audited. Use at your own risk, and avoid using on mainnet with real funds.

Supports sponsoring 4337 `UserOps` with Tornado Cash notes. Uses the [`privacy-paymaster`](https://github.com/ethereum/kohaku-contracts/tree/master/privacy-paymaster) contracts from `kohaku-contracts`.

## Example

```rust,no_run
# use alloy::{providers::DynProvider, signers::local::PrivateKeySigner};
# use kohaku_tornadocash::{
#     merkle_tree::{MerkleTree, MerkleTreeExt},
#     Note,
#     Pool,
#     Withdrawal,
# };
# use kohaku_account_checks::{is_address_unused, is_sender_unused};
# use kohaku_tornadocash_paymaster::UserOperationPaymasterExt;
# use kohaku_userop_kit::{
#     builder::UserOperationBuilder,
#     bundler::{Bundler, pimlico::PimlicoBundler},
#     smart_account::{SmartAccount, simple_7702_smart_account::{Call, Simple7702SmartAccount}},
# };
#
# async fn example(
#     provider: DynProvider,
#     tree: &MerkleTree,
#     pool: Pool,
#     note: Note,
#     allow_used: bool,
#     rng: &mut impl rand::CryptoRng,
# ) -> Result<(), Box<dyn std::error::Error>> {
let owner = PrivateKeySigner::random();
let bundler = PimlicoBundler::new("https://bundler.pimlico.com".parse()?);

let smart_account = Simple7702SmartAccount::new(provider.clone(), owner.address(), pool.chain_id);
if !allow_used {
    is_address_unused(&provider, smart_account.address()).await?;
    is_sender_unused(&provider, &smart_account.entry_point(), &smart_account.address()).await?;
}
let builder = UserOperationBuilder::new_with_smart_account(&smart_account)
    .await?
    .with_call(&vec![Call::default()]);

// The note pays for gas, and its remainder is withdrawn to `owner` during validation, so the
// operation's own calls can spend it.
let withdrawal_merkle_proof = tree.note_proof(&note)?;
let withdrawal = Withdrawal::new(&pool, note, owner.address());
let builder = builder
    .with_tornado_paymaster(withdrawal, &withdrawal_merkle_proof, &provider, &bundler, rng)
    .await?;

let userop = builder.build().sign(&owner).await?;
let hash = bundler.send_user_operation(&userop).await?;
println!("{hash:?}");

Ok(())
# }
```


## Delegated and deployed execution accounts

The existing builder accepts either `Simple7702SmartAccount` (including
`.with_implementation(...)`) or `smart_account::simple_account::SimpleAccount` from `kohaku-userop-kit`.
Both use the same v0.8 execution-call format and fee/proof path. Use the account's
address as the payout recipient when the `UserOp` makes calls. For a deployed account,
the owner's `signTypedData` signature authorizes execution without EIP-7702 signing;
see the userop-kit execution-account example. No paymaster or account Solidity is
added or modified by this integration.

Account reuse policy belongs to the privacy application. Set `allow_used` to false
by default and apply both checks before preparing proofs or signatures. An
explicit opt-in can skip them for a known, recoverable existing account. These
helpers inspect current code/delegation, ETH, native nonce, and `EntryPoint` nonce
key zero; they do not prove an address has no historical activity or token balances.
