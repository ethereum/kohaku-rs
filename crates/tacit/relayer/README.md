# kohaku-tacit-relayer

A client for Tacit pool relayers (keepers). A relayer submits a user's proven spend and is paid from the shielded funds in the same transaction. The user's account never appears on chain. The relayer cannot change the recipient, amount, relayer address or fee: the proof binds them, and the pool recomputes the binding.

- `quote` returns the relayer's fee and address. It is refused if it names another chain or pool, an address other than the configured one, or a fee above the configured ceiling.
- `proving_tree` returns the wallet's tree plus the insertions the relayer has queued after the pool's head, if they lead to the tail the relayer announces. A spend proven against it takes the next free slot instead of waiting for the queue to land.
- `relay` submits a `RelayTx` and returns the transaction hash. When the relayer gives no answer, the error says so: the spend may still have been sent.
- `sync_from_feed` reads confirmed history from the relayer's `/events` feed. The wallet checks it against the pool before keeping it.

```rust,no_run
use kohaku_tacit::{Account, Pool, Wallet, circuit::Prover};
use kohaku_tacit_relayer::RelayerClient;

# async fn example(
#     provider: impl alloy::providers::Provider + Clone,
#     mut wallet: Wallet,
#     prover: Prover,
#     to: kohaku_tacit::TacitAddress,
# ) -> Result<(), Box<dyn std::error::Error>> {
let relayer = RelayerClient::new(wallet.pool())?;
wallet.sync(&provider).await?;
let quote = relayer.quote().await?;
let tree = relayer.proving_tree(&mut wallet, &provider).await;
let mut rng = rand::rng();
let tx = wallet.transfer(&to, 10_u128.pow(16), quote.payer(), &tree, &mut rng)?;
let hash = relayer.relay(&tx.prove(&prover, &mut rng)?.relay_request()).await?;
# Ok(())
# }
```
