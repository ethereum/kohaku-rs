# Changelog

All notable changes to `kohaku-tornadocash-syncer` are documented here. This crate follows [Semantic Versioning](https://semver.org); while in `0.y.z`, breaking changes and features bump the minor version and fixes bump the patch version.

## 0.0.2 - UNRELEASED

### Fixed

- `SagaSyncer` returns an HTTP error for a 4xx or 5xx manifest or chunk response, instead of parsing the body as JSON or gzip ([#148](https://github.com/ethereum/kohaku-rs/pull/148))

## 0.0.1 - 2026-10-25

Initial release.
