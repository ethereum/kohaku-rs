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

## POI availability

With POI enabled, `notes` and `balance` expose `Unavailable` when a status lookup
fails (transport, RPC, malformed response, or incomplete status map). It is a
local observation state, not a POI node status, and never means `Missing`.
Only a completed lookup can establish `Missing`; only `Valid` notes are selected
for spending. If the valid set is insufficient and the requested account/asset
has unavailable notes, build returns `RailgunProviderError::PoiStatusUnavailable`
before proving. An unrelated account/asset outage does not replace a genuine
insufficient-balance error, and sufficient `Valid` notes remain usable.
`None` continues to mean POI is disabled. Callers should display `Unavailable`
as unknown and must not use it to authorize proof resubmission.
