# kohaku-railgun-paymaster

Opt-in RAILGUN paymaster and ERC-4337 UserOperation construction for
[`kohaku-railgun`](../railgun/).

Use `RailgunUserOps::new(&mut wallet, chain_provider)` for a core wallet.
For a PPOI-enabled wallet, enable `kohaku-railgun-poi/userop` and use
`PoiWallet::user_operations(chain_provider)` so note selection and proving
honor the locally validated PPOI lists.

`simulate_gas_limits` builds a dummy proof under a verifier state override;
`prepare_userop_single_proof` generates the real proof after the caller fixes
the gas limits. The crate does not send a UserOperation or persist wallet state.


## Delegated and deployed execution accounts

The existing builder accepts either `Simple7702SmartAccount` (including
`.with_implementation(...)`) or `DeployedSimpleAccount` from `kohaku-userop-kit`.
Both use the same v0.8 execution-call format and fee/proof path. Use the account's
address as the payout recipient when the UserOp makes calls. For a deployed account,
the owner's `signTypedData` signature authorizes execution without EIP-7702 signing;
see the userop-kit execution-account example. No paymaster or account Solidity is
added or modified by this integration.
