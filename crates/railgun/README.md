# kohaku-railgun

Core Rust RAILGUN wallet and transaction APIs: account registration, UTXO indexing,
note queries, shielding, spend selection, and transaction proof generation.

The optional extensions are separate crates:

- [`kohaku-railgun-sync`](../railgun-sync/) provides RPC, Subsquid, and chained syncers.
- [`kohaku-railgun-poi`](../railgun-poi/) adds local PPOI lists, TXID indexing, proof recovery, and submission.
- [`kohaku-railgun-paymaster`](../railgun-paymaster/) prepares paymaster-backed ERC-4337 operations.
- [`kohaku-railgun-wasm`](../railgun-wasm/) exposes address and shield-preparation bindings to JavaScript.

The core wallet does not contact a PPOI service or prepare paymaster operations.
Consumers choose a `UtxoSyncer` and supply it through
`RailgunBuilder::with_utxo_syncer`. `RailgunWallet::transactions()` selects and
proves spends against indexed UTXOs. `Spend::try_new(signer, notes)` constructs a
spend from explicit notes, and `ShieldBuilder` constructs a shield.

## State and persistence

The SDK accepts typed state and returns typed state. It does not choose a database,
write checkpoints, or require a particular serialization format. Start with
`RailgunCheckpoint::default()` or load one through your application's storage
layer, pass it to `RailgunBuilder::with_state`, and call `wallet.state()` after
state-changing operations. Persist the result atomically before treating it as
a durable sync checkpoint. Register signers again after restoring; keys are not
part of the checkpoint.

`RailgunCheckpoint` contains UTXO trees, account notes, and the sync cursor.
PPOI has a separate checkpoint in `kohaku-railgun-poi`. Checkpoints contain
unencrypted notes, so protect them as wallet data and keep separate state for
each chain and wallet. Existing combined Railgun checkpoints must be rebuilt
by resyncing.

## Examples

See the [integration tests](./tests/integration/).
