# Retrieve cached pool events

`RemoteSyncer` reads pool events from a [compatible remote cache](../../syncer/src/remote.rs)
and returns a snapshot of the actual covered block range.

With the generated bindings loaded and caller-supplied `baseUrl`, `pool`,
`fromBlock` and `toBlock`:

```js
const remote = new RemoteSyncer(baseUrl);
let snapshot;
try {
  snapshot = await remote.syncRange(pool, fromBlock, toBlock);
  console.log(snapshot.fromBlock, snapshot.toBlock, snapshot.events);
} finally {
  snapshot?.free();
  remote.free();
}
```
