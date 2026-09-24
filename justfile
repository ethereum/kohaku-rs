check:
    cd crates && cargo check --all-targets --all-features
    cd crates && cargo clippy --all-targets --all-features -- -D warnings

test: check
    cd crates && cargo test --release --all-targets --all-features
    cd crates && cargo test --release --all-targets --all-features -- --ignored

hegota-deploy:
    cd crates && HEGOTA_RPC_URL="${HEGOTA_RPC_URL:-}" HEGOTA_DEPLOYER_PK="${HEGOTA_DEPLOYER_PK:-}" ALLOW_TESTBED_SETUP="${ALLOW_TESTBED_SETUP:-1}" cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- deploy

hegota-deploy-accounts:
    cd crates && HEGOTA_RPC_URL="${HEGOTA_RPC_URL:-}" HEGOTA_DEPLOYER_PK="${HEGOTA_DEPLOYER_PK:-}" cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- deploy-accounts

hegota-shield *args:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- shield {{args}}

hegota-publish:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- publish

hegota-unshield *args:
    cd crates && CIRCUIT_ARTIFACTS="${CIRCUIT_ARTIFACTS:-minimal-shield/.hegota-data/circuit}" cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- unshield {{args}}

hegota-unshield-with-tail *args:
    cd crates && CIRCUIT_ARTIFACTS="${CIRCUIT_ARTIFACTS:-minimal-shield/.hegota-data/circuit}" cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- unshield-with-tail {{args}}

hegota-unshield-for-gas *args:
    cd crates && CIRCUIT_ARTIFACTS="${CIRCUIT_ARTIFACTS:-minimal-shield/.hegota-data/circuit}" cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- unshield-for-gas {{args}}
