# kohaku-railgun-sync

Concrete syncers for [`kohaku-railgun`](../railgun/): `RpcSyncer`,
`SubsquidSyncer`, and `ChainedSyncer`. The core crate keeps the `UtxoSyncer`
and `TxidSyncer` traits, so consumers may provide their own implementations.

`default_utxo_syncer(&chain, provider)` composes Subsquid history with an RPC
tail. Pass the resulting syncer to `RailgunBuilder::with_utxo_syncer`. PPOI users
also supply a TXID syncer to `PoiWallet::new`.

Syncing does not persist state. Export the core wallet checkpoint after sync.
