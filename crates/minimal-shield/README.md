# kohaku-minimal-shield

Hegota client for the [minimal-shielded-pool](../../../minimal-shielded-pool) join-split.

Unshield is a **three- or four-frame** pool-as-sender FrameTx (`position-notes-v2`):

1. `VERIFY(0x8272, 72-byte recent-root tuple)` (wallet default ~8k gas)
2. `VERIFY(pool, 288-byte Groth16 proof + beta)` with payment approval (~225k gas)
3. `SENDER(pool, settle)`
4. Optional leftover `DEFAULT` (mode 0, value 0). The proof `recipient` is the
   payout destination, not the frame target. The leftover may target the pool
   (e.g. `publishEpochRoot` after a change note). Multicall3 is only used when
   several calls must share that single DEFAULT.

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
just hegota-shield --value 500000000000000000
just hegota-unshield --recipient 0x...
just hegota-unshield-with-tail --owner-pk 0x... --to 0x...
just hegota-unshield-for-gas --owner-pk 0x... --to 0x...
```

`--owner-pk` is required for the tail commands. `--amount` defaults to 0.001 ETH.
`--to` defaults to the deployer so the inner transfer is not a new EOA.
