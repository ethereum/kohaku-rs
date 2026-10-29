# tornadocash-relayer

Client for interacting with Tornadocash relayers.

Tornadocash relayers are services run by third parties that accept and submit withdrawal proofs on behalf of users. They allow users to withdraw from Tornadocash to a fresh address that has no ETH to pay for gas at the cost of a small fee. Relayers are run by independent parties, and the Tornadocash DAO does not endorse any particular relayers.

See [tornado-relayer](https://github.com/tornado-dao/tornado-relayer/tree/mainnet-v5) for the reference implementation.

## Example

```rust,no_run
# use alloy::{
#     primitives::{Address, U256},
#     providers::{DynProvider, Provider},
# };
# use kohaku_tornadocash::{
#     merkle_tree::{MerkleTree, MerkleTreeExt},
#     Note,
#     Pool,
#     Withdrawal,
# };
# use kohaku_tornadocash_relayer::Relayer;
#
# async fn example(
#     provider: DynProvider,
#     tree: &MerkleTree,
#     pool: Pool,
#     note: Note,
#     recipient: Address,
#     rng: &mut impl rand::CryptoRng,
# ) -> Result<(), Box<dyn std::error::Error>> {
let relayer = Relayer::new("https://mainnet.relayer.com");

let gas_price = provider.get_gas_price().await?;
let payer = relayer.quote(&pool, gas_price, U256::ZERO).await?;
let merkle_proof = tree.note_proof(&note)?;

let withdrawal = Withdrawal::new(&pool, note, recipient)
    .with_payer(payer)
    .prove(&merkle_proof, rng)?;

// Confirmation is judged by the nullifier being spent on-chain, not by the relayer's report.
let receipt = relayer.withdraw(withdrawal).await?;
let tx_hash = relayer.await_confirmation(&provider, &receipt).await?;
println!("{tx_hash:?}");

# Ok(())
# }
```