# Changelog

All notable changes to `kohaku-tornadocash-wasm` are documented here. This crate follows [Semantic Versioning](https://semver.org); while in `0.y.z`, breaking changes and features bump the minor version and fixes bump the patch version.

## 0.0.2 - 2026-10-27

### Removed

- **Breaking:** Removed `PaymasterInfo` struct ([#105](https://github.com/ethereum/kohaku-rs/pull/105))

### Changed

- **Breaking:** Changed type of `Pool::paymaster` from `Option<PaymasterInfo>` to `Option<Address>` ([#105](https://github.com/ethereum/kohaku-rs/pull/105))

## 0.0.1 - 2026-10-25

Initial release.
