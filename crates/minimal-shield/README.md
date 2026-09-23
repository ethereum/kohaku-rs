# kohaku-minimal-shield

Hegota client for the [minimal-shielded-pool](../../../minimal-shielded-pool) join-split.

Unshield is a **three- or four-frame** pool-as-sender FrameTx (`generic-tail-v1`):

1. `VERIFY(0x8272, 72-byte recent-root tuple)`
2. `VERIFY(pool, 256-byte Groth16 proof)` with payment approval
3. `SENDER(pool, settle)`
4. Optional leftover `DEFAULT` (mode 0, value 0). The proof `recipient` is the
   payout destination, not the frame target.

EOA withdraw: leftover `claimWithdrawal(recipient)` targeting the pool.

FrameAccount transfer: leftover `Multicall3.aggregate3([createAccount(owner, salt),
claimWithdrawal(account), executeBatch(calls, sig)])`. That order is fixed:
deploy, then claim, then execute. Deploy before claim so Hegotá `CREATE2` does
not hit a balance-only account. The leftover `DEFAULT` target is Multicall3;
the last inner call is `FrameAccount.executeBatch`, which sends ETH to `--to`.
The proof `recipient` is the precomputed account. Salt is
always `keccak256(abi.encodePacked("FRAMEACCT1", owner))` — one owner, one
account per factory. The account is CREATE2'd if missing. Tail gas is estimated before the proof
(15% pad, with first-deploy floors) so `max_cost` cannot undershoot and the
proof-bound fee stays tied to that pin. `unshield-for-gas` sets public amount
to 0 and spends the note on the fee, then runs `executeBatch` on the account.

State for the `hegota` CLI lives in `.hegota-data/` (gitignored).

```
HEGOTA_RPC_URL=... HEGOTA_DEPLOYER_PK=... ALLOW_TESTBED_SETUP=1 \
  MSP_ROOT=../minimal-shielded-pool FRAME_ACCT_ROOT=../frame-privacy-acct \
  MSP_CIRCUIT_ARTIFACTS=... \
  just hegota-deploy
just hegota-deploy-accounts
just hegota-shield --value 500000000000000000
just hegota-unshield --recipient 0x...
just hegota-unshield-with-tail --owner-pk 0x... --to 0x...
just hegota-unshield-for-gas --owner-pk 0x... --to 0x...
```

`hegota deploy-accounts` forge-creates `FrameAccountFactory` only (reuses the
existing Multicall3) and measures CREATE2 gas. Do not rerun full `hegota deploy`
against a live pool. `--owner-pk` is required. `--amount` defaults to 0.001 ETH.
`--to` defaults to the deployer so the inner transfer is not a new EOA.
