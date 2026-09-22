check:
    cd crates && cargo check --all-targets --all-features
    cd crates && cargo clippy --all-targets --all-features -- -D warnings

test: check
    cd crates && cargo test --release --all-targets --all-features
    cd crates && cargo test --release --all-targets --all-features -- --ignored

hegota-deploy:
    cd crates && HEGOTA_RPC_URL="${HEGOTA_RPC_URL:-}" HEGOTA_DEPLOYER_PK="${HEGOTA_DEPLOYER_PK:-}" ALLOW_TESTBED_SETUP="${ALLOW_TESTBED_SETUP:-1}" cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- deploy

hegota-shield *args:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- shield {{args}}

hegota-unshield *args:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- unshield {{args}}

hegota-unshield-4337 *args:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- unshield-4337 {{args}}
