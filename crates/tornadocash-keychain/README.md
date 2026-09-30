# kohaku-tornadocash-keychain

Deterministic note derivation for [`kohaku-tornadocash`](../tornadocash/).

Keychains are used to derive tornadocash note secrets from a single source of entropy. This makes it easier for users to manage their notes, because they only need to backup a single secret (e.g. a mnemonic) instead of each individual note secret. New notes can be derived from the keychain by incrementing a nonce, and notes can be recovered by scanning a pool's synced events against the keychain's derivation scheme.

> [!WARNING]
> This crate is **not usable with real funds yet**. The derivation scheme is a placeholder
> and thus constant & insecure. It is only intended for testing and development purposes.

## Examples

### Deposit

```rust,no_run
use kohaku_tornadocash_keychain::{DynKeychain, KeychainExt};

async fn example(
    keychain: &DynKeychain,
    pool: &kohaku_tornadocash::Pool,
    nonce: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let deposit = keychain.deposit(pool, nonce).await?;

    Ok(())
}
```

### Withdraw

```rust,no_run
use kohaku_tornadocash::withdrawal::Withdrawal;
use kohaku_tornadocash_keychain::{DynKeychain, KeychainExt};

async fn example(
    keychain: &DynKeychain,
    pool: &kohaku_tornadocash::Pool,
    nonce: u64,
    recipient: alloy::primitives::Address,
) -> Result<(), Box<dyn std::error::Error>> {
    let note = keychain.note(pool, nonce).await?;
    let withdrawal = Withdrawal::new(pool, note.note, recipient);

    Ok(())
}
```

### Note Recovery

Scans a pool's synced events for notes this keychain derived.

Note recovery can discover the lower bound on a keychain's consumed nonces. Note recovery can't guarantee that all notes have been discovered. Notes will be missing if they aren't present in the synced events. For example, if a withdrawal has been proven but not yet submitted on-chain, the withdrawal event won't be discovered.

```rust,no_run
use kohaku_tornadocash::syncer::Syncer;
use kohaku_tornadocash_keychain::{DynKeychain, recover};

async fn example(
    keychain: &DynKeychain,
    syncer: &kohaku_tornadocash::DynSyncer,
    pool: &kohaku_tornadocash::Pool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = syncer.sync(pool, ..).await?;
    let notes = recover(keychain, pool, &snapshot.events, None).await?;

    for note in &notes {
        println!("{}: leaf {}", note.nonce, note.deposit.leaf_index);
    }

    let next_nonce = notes.last().map(|note| note.nonce + 1).unwrap_or(0);
    Ok(())
}
```

