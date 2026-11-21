# kohaku-tornadocash-circuit

Tornadocash zk circuits for Kohaku.

Wraps [`websnark-rs`](https://crates.io/crates/websnark-rs) with a simple API for proof generation.

## Benchmarks

Benchmarks were run on a Ryzen 5 3600, 32GB RAM.

| Method | Target           | Time (ms) |
| ------ | ---------------- | --------- |
| prove  | native           | 1,442     |
| prove  | native +parallel | 485.72    |
