# kohaku-merkle-tree

Generic binary Merkle tree implementation in rust. Uses [`kohaku-kv-store`](../kv-store/) as the underlying storage engine.

## Benchmarks

Benchmarks were run on a Ryzen 5 3600, 32GB RAM.

| Method               | Target | Time (ms) |
| -------------------- | ------ | --------- |
| merkle_insert/100    | native | 0.056     |
| merkle_insert/10_000 | native | 6.531     |
