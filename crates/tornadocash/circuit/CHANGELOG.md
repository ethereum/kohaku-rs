# Changelog

All notable changes to `kohaku-tornadocash-circuit` are documented here. This crate follows [Semantic Versioning](https://semver.org); while in `0.y.z`, breaking changes and features bump the minor version and fixes bump the patch version.

## 0.2.0 - UNRELEASED

### Added

- `CircuitArtifacts` and `CircuitArtifactsError` ([#145](https://github.com/ethereum/kohaku-rs/pull/145))

### Changed

- **Breaking:** The circuit and proving key are no longer embedded. `prove` takes `&CircuitArtifacts` ([#145](https://github.com/ethereum/kohaku-rs/pull/145))

## 0.1.0 - 2026-10-25

Initial release.
