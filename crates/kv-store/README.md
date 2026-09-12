# kohaku-kv-store

A simple key-value store implementation in Rust.

## Examples

```rust
#[tokio::main]
async fn main() {
    let store: kohaku_kv_store::Store = kohaku_kv_store::memory::MemoryStore::new().into();

    // Put values into the store
    store.put("key1", "value1").await;
    store.put("key2", "value2").await;

    // Retrieve values from the store
    assert_eq!(store.get("key1").await, Some(b"value1".to_vec()));
    assert_eq!(store.get("key3").await, None);

    // Delete keys from the store
    store.delete("key1").await;
    assert_eq!(store.get("key1").await, None);

    // Scope storage to specific namespaces
    let scoped_store = store.scope("namespace1");
    scoped_store.put("key2", "value2.2").await;

    assert_eq!(store.get("key2").await, Some(b"value2".to_vec()));
    assert_eq!(scoped_store.get("key2").await, Some(b"value2.2".to_vec()));
}
```
