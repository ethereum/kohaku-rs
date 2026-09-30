# kohaku-tornadocash-wallet

Deterministic note management for [`kohaku-tornadocash`](../tornadocash/).

> [!WARNING]
> This crate is **not usable with real funds yet**. The derivation scheme is a placeholder
> and thus constant & insecure. It is only intended for testing and development purposes.

## Examples

### Deposit

```rust,no_run
use kohaku_tornadocash_wallet::ext::DepositWalletExt;

# async fn example(
#     provider: &kohaku_tornadocash::provider::TornadoProvider,
#     wallet: &kohaku_tornadocash_wallet::wallet::Wallet,
#     pool: kohaku_tornadocash::pool::Pool,
# ) -> Result<(), Box<dyn std::error::Error>> {
let deposit = provider
    .deposit(pool, &mut rand::rng())
    .await
    .with_wallet(wallet)
    .await?;
# Ok(())
# }
```

### Withdraw

```rust,no_run
# async fn example(
#     provider: &kohaku_tornadocash::provider::TornadoProvider,
#     wallet: &kohaku_tornadocash_wallet::wallet::Wallet,
#     pool: &kohaku_tornadocash::pool::Pool,
#     recipient: alloy::primitives::Address,
# ) -> Result<(), Box<dyn std::error::Error>> {
let nonce = 0;

let note = wallet.note(pool, nonce).await?;
let withdrawal = provider.withdraw(note, recipient);
# Ok(())
# }
```

### List Notes

```rust,no_run
# async fn example(
#     provider: &kohaku_tornadocash::provider::TornadoProvider,
#     wallet: &kohaku_tornadocash_wallet::wallet::Wallet,
#     pool: &kohaku_tornadocash::pool::Pool,
# ) -> Result<(), Box<dyn std::error::Error>> {
for note in wallet.notes(pool).await? {
    println!("{}: {:?}", note.nonce, note.status);
}
# Ok(())
# }
```
