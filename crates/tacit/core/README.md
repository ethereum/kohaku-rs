# kohaku-tacit

A client for the Tacit pool on Ethereum. The pool is one immutable contract with no owner, pause or upgrade. It holds ETH in notes of any amount: shield, pay privately inside the pool, and withdraw any part of a balance. Proofs are made on the user's device.

The crate covers:

- **Keys and addresses.** One 32-byte identity key opens an [`Account`]. Kohaku derives it at `m/29793'/1'/{account}'`. Every Tacit app derives the same keys from it, so the balance is the same everywhere. Addresses are bech32m `bp1…` strings.
- **Notes.** Each output carries a memo encrypted to the recipient's view key. A wallet finds its notes by opening the memos.
- **Sync.** [`Wallet::sync`] reads `Transact` logs, keeps the pool's tree incrementally, and checks every batch against the pool's own record of its roots.
- **Operations.** [`Wallet::shield`], [`Wallet::transfer`], [`Wallet::unshield`] and [`Wallet::consolidate`] build a [`Transaction`]. Proving it with [`circuit::Prover`] gives a [`ProvenTransaction`]. Send its [`ProvenTransaction::call`] from the user's account, or hand its [`ProvenTransaction::relay_request`] to a relayer (`kohaku-tacit-relayer`).

## Example

```rust,no_run
use kohaku_tacit::{Account, Payer, Pool, Wallet, circuit::Prover};

# async fn example(
#     provider: impl alloy::providers::Provider + Clone,
#     identity: [u8; 32],
#     wasm: &[u8],
#     zkey: &[u8],
# ) -> Result<(), Box<dyn std::error::Error>> {
let mut wallet = Wallet::new(Account::from_identity(&identity)?, Pool::ethereum());
wallet.sync(&provider).await?;
println!("{} holds {} wei", wallet.address(), wallet.balance());

let prover = Prover::new(wasm, zkey)?;
let mut rng = rand::rng();
let to = "0x1111111111111111111111111111111111111111".parse()?;
let tx = wallet.unshield(to, 10_u128.pow(16), Payer::SELF, wallet.tree(), &mut rng)?;
let call = tx.prove(&prover, &mut rng)?.call();
// send `call` from the user's account
# Ok(())
# }
```

A proof binds the tree it was built against. A transaction that inserts notes is good only while the pool's head is unchanged. If another transaction lands first, sync and build it again.

## Pairing with fixed denominations

A fixed-denomination pool can only take multiples of its denominations, so an arbitrary amount never fits: the leftover stays public, waits, or goes into a rarely used denomination whose anonymity set is thin. This pool takes any amount, so it can hold the leftover instead.

With the `tornado` feature, [`pairing::Split`] plans that division: the round part into Tornado Cash's deep pools, the rest here.

```rust
# #[cfg(feature = "tornado")]
# fn example() {
use kohaku_tacit::pairing::{Split, SplitOptions};

// 1.37 ETH on Ethereum: one 1 ETH note, three 0.1 ETH notes, 0.07 ETH left over.
let split = Split::for_tornado(1_370_000_000_000_000_000, 1, &SplitOptions::default());
assert_eq!(split.deposits.len(), 4);
assert_eq!(split.remainder, 70_000_000_000_000_000);
# }
```

## Features

- `mnemonic`: [`Account::from_mnemonic`], the account Kohaku's keystore opens from a BIP-39 phrase.
- `parallel`: parallel proving.
- `tornado`: [`pairing::Split`], planning a split between Tornado Cash's denominations and this pool.
