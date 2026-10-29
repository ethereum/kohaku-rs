# tornadocash-paymaster

Client for interacting with Tornado Cash paymasters.

> [!IMPORTANT]
> The privacy-paymaster contracts are experimental and not audited. Use at your own risk, and avoid using on mainnet with real funds.

Supports sponsoring 4337 UserOps with Tornado Cash notes. Uses the [`privacy-paymaster`](https://github.com/ethereum/kohaku-contracts/tree/master/privacy-paymaster) contracts from `kohaku-contracts`.

## Example

```rust,no_run
# use alloy::{providers::DynProvider, signers::local::PrivateKeySigner};
# use kohaku_tornadocash::{
#     merkle_tree::{MerkleTree, MerkleTreeExt},
#     Note,
#     Pool,
#     Withdrawal,
# };
# use kohaku_tornadocash_paymaster::UserOperationPaymasterExt;
# use kohaku_userop_kit::{
#     builder::UserOperationBuilder,
#     bundler::{Bundler, pimlico::PimlicoBundler},
#     smart_account::simple_7702_smart_account::{Call, Simple7702SmartAccount},
# };
#
# async fn example(
#     provider: DynProvider,
#     tree: &MerkleTree,
#     pool: Pool,
#     note: Note,
#     rng: &mut impl rand::CryptoRng,
# ) -> Result<(), Box<dyn std::error::Error>> {
let owner = PrivateKeySigner::random();
let bundler = PimlicoBundler::new("https://bundler.pimlico.com".parse()?);

let smart_account = Simple7702SmartAccount::new(provider.clone(), owner.address(), pool.chain_id);
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
