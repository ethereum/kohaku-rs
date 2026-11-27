# Changelog

All notable changes to `kohaku-tornadocash-syncer` are documented here. This crate follows [Semantic Versioning](https://semver.org); while in `0.y.z`, breaking changes and features bump the minor version and fixes bump the patch version.

## 0.0.2 - UNRELEASED

### Changed

- **Breaking:** `Syncer::sync_range` and `Syncer::sync` take a slice of pools and return one `Snapshot` per pool, in the same order ([#155](https://github.com/ethereum/kohaku-rs/pull/155))

## 0.0.1 - 2026-10-25

Initial release.
