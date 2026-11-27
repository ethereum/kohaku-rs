# tornadocash-paymaster

Client for interacting with Tornado Cash paymasters.

> [!IMPORTANT]
> The privacy-paymaster contracts are experimental and not audited. Use at your own risk, and avoid using on mainnet with real funds.

Supports sponsoring 4337 `UserOps` with Tornado Cash notes. Uses the [`privacy-paymaster`](https://github.com/ethereum/kohaku-contracts/tree/master/privacy-paymaster) contracts from `kohaku-contracts`.

## Example

```rust,no_run
# use alloy::{
#     providers::{DynProvider, RootProvider},
#     signers::local::PrivateKeySigner,
# };
# use kohaku_tornadocash::{
#     merkle_tree::{MerkleTree, MerkleTreeExt},
#     Note,
#     Pool,
#     Withdrawal,
# };
# use kohaku_tornadocash_circuit::CircuitArtifacts;
# use kohaku_tornadocash_paymaster::UserOperationPaymasterExt;
# use kohaku_userop_kit::{
#     builder::UserOperationBuilder,
#     bundler::BundlerApi,
#     smart_account::{
#         Call,
#         simple_7702_smart_account::{SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08, Simple7702SmartAccount},
#     },
# };
#
# async fn example(
#     provider: DynProvider,
#     tree: &MerkleTree,
#     artifacts: &CircuitArtifacts,
#     pool: Pool,
#     note: Note,
#     rng: &mut impl rand::CryptoRng,
# ) -> Result<(), Box<dyn std::error::Error>> {
let owner = PrivateKeySigner::random();
let bundler = RootProvider::new_http("https://bundler.pimlico.com".parse()?);

let smart_account = Simple7702SmartAccount::new(
    provider.clone(),
    owner.address(),
    SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08,
    pool.chain_id,
);
let builder = UserOperationBuilder::new_with_smart_account(&smart_account)
    .await?
    .with_call(&vec![Call::default()]);

// The note pays for gas, and its remainder is withdrawn to `owner` during validation, so the
// operation's own calls can spend it.
let withdrawal_merkle_proof = tree.note_proof(&note)?;
let withdrawal = Withdrawal::new(&pool, note, owner.address());
let builder = builder
    .with_tornado_paymaster(
        withdrawal,
        &withdrawal_merkle_proof,
        artifacts,
        &provider,
        &bundler,
        rng,
    )
    .await?;

let userop = builder.build().sign(&owner).await?;
let receipt = bundler.send_user_operation(&userop).await?.get_receipt().await?;
println!("{receipt:?}");

Ok(())
# }
```
