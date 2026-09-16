check:
    cd crates && cargo check --workspace --all-targets --all-features --exclude kohaku-pir-provider
    cd crates && cargo check --all-targets -p kohaku-pir-provider
    cd crates && cargo clippy --workspace --all-targets --all-features --exclude kohaku-pir-provider -- -D warnings
    cd crates && cargo clippy --all-targets -p kohaku-pir-provider -- -D warnings

test: check
    cd crates && cargo test --release --workspace --all-targets --all-features --exclude kohaku-pir-provider
    cd crates && cargo test --release --all-targets -p kohaku-pir-provider
    cd crates && cargo test --release --workspace --all-targets --all-features --exclude kohaku-pir-provider -- --ignored
    cd crates && cargo test --release --all-targets -p kohaku-pir-provider -- --ignored
