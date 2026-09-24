check:
    cd crates && cargo check --all-targets --all-features
    cd crates && cargo clippy --all-targets --all-features -- -D warnings

test: check
    cd crates && cargo test --release --all-targets --all-features
    cd crates && cargo test --release --all-targets --all-features -- --ignored

hegota-shield *args:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- shield {{args}}

hegota-publish:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- publish

hegota-unshield *args:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- unshield {{args}}

hegota-unshield-with-tail *args:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- unshield-with-tail {{args}}

hegota-unshield-for-gas *args:
    cd crates && cargo run -p kohaku-minimal-shield --features hegota --bin hegota -- unshield-for-gas {{args}}
