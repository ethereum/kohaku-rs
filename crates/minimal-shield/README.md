# kohaku-minimal-shield

Hegota client for the [minimal-shielded-pool](../../../minimal-shielded-pool) join-split.

Unshield is a **three- or four-frame** pool-as-sender FrameTx (`generic-tail-v1`):

1. `VERIFY(0x8272, 72-byte recent-root tuple)`
2. `VERIFY(pool, 256-byte Groth16 proof)` with payment approval
3. `SENDER(pool, settle)`
4. Optional leftover `DEFAULT` (mode 0, value 0). The proof `recipient` is the
   payout destination, not the frame target.

EOA withdraw: leftover `claimWithdrawal(recipient)` targeting the pool.

4337 dummy transfer: leftover `Multicall3.aggregate3([claimWithdrawal(account),
handleOps([userOp], account)])`. The UserOp deploys a fresh SimpleAccount via
`initCode` and `execute`s an ETH transfer. UserOp `gasFees` are zero so
EntryPoint does not re-charge the unshielded ETH; the FrameTx fee still prepays
`max_cost`.

State for the `hegota` CLI lives in `.hegota-data/` (gitignored).

```
HEGOTA_RPC_URL=... HEGOTA_DEPLOYER_PK=... ALLOW_TESTBED_SETUP=1 \
  MSP_ROOT=../minimal-shielded-pool FRAME_ACCT_ROOT=../frame-privacy-acct \
  MSP_CIRCUIT_ARTIFACTS=... \
  just hegota-deploy
just hegota-shield --value 500000000000000000
just hegota-unshield --recipient 0x...
just hegota-unshield-4337 --to 0x... --amount 10000000000000000
```
