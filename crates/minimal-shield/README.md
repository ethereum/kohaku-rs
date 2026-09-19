# kohaku-minimal-shield

Hegota client for the [minimal-shielded-pool](../../../minimal-shielded-pool) join-split.

Unshield is always the same **five-frame** pool-as-sender FrameTx:

1. `VERIFY(0x8272, tuple)`
2. `VERIFY(pool, 256-byte proof)`
3. `SENDER(pool, settle)`
4. `SENDER(pool, ensureAndClaim)` — `factory == 0` skips CREATE2
5. `SENDER(recipient, executeBatch)` — empty calls is a no-op

State for the `hegota` CLI lives in `.hegota-data/` (gitignored).

```
HEGOTA_RPC_URL=... HEGOTA_DEPLOYER_PK=... ALLOW_TESTBED_SETUP=1 just hegota-deploy
just hegota-shield
just hegota-unshield
```
