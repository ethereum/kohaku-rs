# kohaku-fork-kit

Helpers for deploying test fixtures onto a local fork or Hegotá.

`deploy_minimal_shield_pool` runs MSP `devnet/run_live_dispatcher.sh` (`SPEND=0`)
and `frame-privacy-acct` `script/Deploy.s.sol` (`Multicall3`, `EntryPoint`,
`SimpleAccountFactory`). It does not use the canonical CREATE2 `EntryPoint`
address; Hegotá needs a fresh `CREATE`.
