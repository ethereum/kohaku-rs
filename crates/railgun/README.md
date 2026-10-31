# kohaku-railgun

Rust Railgun client library for Railgun's on-chain and off-chain components. It provides support for:

- UTXO and TXID syncing
- State querying
- Transaction construction
- Proof generation
- POI proof generation
- POI proof submission
- Transaction submission via broadcasters

## Examples

See the [integration tests](./tests/integration/).

## API boundaries

`RailgunBuilder::build()` creates a `RailgunWallet` for account registration,
syncing, checkpoint export, note queries, and POI queue management. Borrow
`wallet.transactions()` to select notes and prove Railgun transactions. Borrow
`wallet.user_operations()` when preparing paymaster-backed ERC-4337 operations.
These helpers use the wallet's current indexed notes and POI status. A shield
can be built with `ShieldBuilder` (or `wallet.shield()` when using the wallet's
chain configuration). To create a spend from specific notes, call
`Spend::try_new(signer, notes)` directly.

This replaces the former `RailgunProvider` API. Callers still own persistence:
they pass a `RailgunCheckpoint` through `RailgunBuilder::with_state` and export
the updated checkpoint with `wallet.state()`.

## POI availability

With POI enabled, status comes from a local copy of each POI list. Sync copies
leaves by index range, so a read does not name the wallet's commitments.
`Unavailable` means that copy is not caught up: it was never synced, or the last
sync did not reach the end of the list. It is a local observation state, not a
POI node status, and never means `Missing`. Only a sync that reached the zero
leaf can establish `Missing` for a commitment that is absent. A commitment
already in the tree is `Valid` even before the tail is confirmed. Only `Valid`
notes are selected for spending. If the valid set is insufficient and the
requested account/asset has unavailable notes, build returns
`RailgunError::PoiStatusUnavailable` before proving. An unrelated
account/asset does not replace a genuine insufficient-balance error, and
sufficient `Valid` notes remain usable. `None` means POI is disabled. Callers
should display `Unavailable` as unknown and must not use it to authorize proof
resubmission. A blocked shield is not queried on its own; once the list is
caught up it is `Missing`.

A copied page becomes visible only after root validation and an atomic update
of the in-memory state. Cancelling acquisition or validation preserves the last
committed checkpoint.

## State and persistence

The SDK accepts typed state and returns typed state. It does not choose a database,
write checkpoints, or require a particular serialization format. Start with
`RailgunCheckpoint::default()` or load one through your application's storage
layer, pass it to `RailgunBuilder::with_state`, and call `wallet.state()` after
state-changing operations. Persist the result atomically before treating it as
a durable sync checkpoint. Register signers again after restoring; keys are not
part of the checkpoint.

`RailgunCheckpoint` contains UTXO trees, account notes, TXID trees and cursors,
POI list copies, and pending POI work. Its fields also let consumers keep only
the parts they use. The indexers stage updates and publish them only after root
validation. POI lists publish each validated batch; after cancellation, a caller
can still export the last published prefix. Checkpoints contain unencrypted
notes and pending proof inputs, so protect them as wallet data. Keep separate
checkpoints for each chain and wallet. Existing opaque `RailgunSnapshot` data is
not compatible with this typed state and must be rebuilt by resyncing.

## Controlling POI proof submission

With POI enabled, `sync()` and `sync_to()` retain the automatic workflow:
synchronize state, then generate and submit pending POI proofs. A wallet that
needs a separate disclosure decision can call `sync_without_poi_submission()`
or `sync_to_without_poi_submission(block)`, then explicitly call
`process_pending_poi()` when appropriate.

The submission-free sync still copies POI lists and checks txid roots. List
reads name leaf ranges, not owned commitments. It is not a no-disclosure scan
of the chain indexer. Explicit processing handles the entire pending queue with
the existing best-effort behavior: individual failures are logged and retained.
`Ok` does not establish acceptance, and a response failure does not establish
that nothing was submitted. Observe the local list copy separately before
spending. Dropping processing mid-POST leaves an unknown outcome; cancelling
after delivery but before the final save can leave persisted work that is
processed again after reopening. These methods do not add per-proof review or a
durable submission-attempt journal.

`notes()`, `balance()` and transaction construction read that local copy when
POI is enabled. They do not query the node. Until a sync has caught the list
up, a commitment that is not already in the tree is `Unavailable`.

The loopback control test uses only public dummy state. When running behind a
system HTTP proxy, include `127.0.0.1,localhost` in `NO_PROXY` so the fixture
traffic stays local.

## Local tests and PPOI service

The regular Railgun tests use disposable Anvil accounts and a loopback POI
fixture; no Sepolia RPC, developer key, or live POI endpoint is required. The
fixture verifies public precomputed proofs but does not reproduce a deployed
PPOI list provider. Fresh proof generation remains an opt-in test using public,
pinned circuit artifacts; see the [fixture instructions](./tests/fixtures/poi-conformance/README.md).

`nix build .#railgunPpoi` packages the upstream PPOI node as an executable at
`result/bin/railgun-ppoi`. This is a separate opt-in service, not part of the
default development shell or the regular test fixture. Running it requires
MongoDB and the node configuration described by the
[upstream project](https://github.com/Railgun-Community/private-proof-of-innocence/tree/4b1eaf6ef19099dbfd6b43b1ca78d2ce0132a752/packages/node).
