# Relayer fee quotes

`RelayerStatus` reads a relayer's status response and quotes withdrawal fees using
the core.

With the generated bindings loaded and caller-supplied `rawStatusJson` and `pool`:

```js
const status = RelayerStatus.parse(rawStatusJson);
let payer;
try {
  if (status.supports(pool)) {
    payer = status.quote(pool, 1_000_000_000n, 0n);
    console.log(payer.address, payer.fee, payer.refund);
  }
} finally {
  payer?.free();
  status.free();
}
```
