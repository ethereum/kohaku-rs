# kohaku-railgun-poi

Opt-in local PPOI list synchronization, TXID indexing, POI proof recovery, and
proof submission for [`kohaku-railgun`](../railgun/). Construct a core
`RailgunWallet` with a chosen UTXO syncer, then wrap it with
`PoiWallet::new(wallet, poi_checkpoint, txid_syncer)`. For a restored session,
pass `PoiWalletCheckpoint::railgun` to `RailgunBuilder::with_state` and its
`poi` field to `PoiWallet::new`. Persist both parts atomically after changes.

Use `PoiWallet::transactions()` for POI-aware selection and proving. It accepts
only locally `Valid` notes and checks every input before requesting a spending
signature. A shortfall involving `Unavailable` notes returns
`PoiWalletError::PoiStatusUnavailable`. The core wallet does not apply this
policy, so keep the `PoiWallet` as the wallet interface for a PPOI-enabled flow.

`sync()` and `sync_to()` copy list pages, validate roots, then generate and
submit pending POI proofs. To choose when proof inputs leave the wallet, call
`sync_without_poi_submission()` or `sync_to_without_poi_submission(block)`, then
`process_pending_poi()`. The latter processes the whole queue on a best-effort
basis. `Ok` does not establish proof acceptance, and a lost response does not
establish that submission failed. List reads name leaf ranges rather than the
wallet's commitments, but sync still contacts the configured services.

The `userop` feature adds `PoiWallet::user_operations(provider)` and applies
the same local note policy to paymaster builds. This feature explicitly opts
into [`kohaku-railgun-userop`](../railgun-userop/).

The checkpoint contains unencrypted notes and pending proof inputs. Protect it
as wallet data. Older combined Railgun checkpoints require a resync.
