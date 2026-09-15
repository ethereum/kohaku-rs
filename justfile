check:
    cd crates && cargo check --all-targets --all-features
    cd crates && cargo clippy --all-targets --all-features -- -D warnings

test: check
    cd crates && cargo test --release --all-targets --all-features
    cd crates && cargo test --release --all-targets --all-features -- --ignored
