# Withdrawal proving

`Withdrawal` proves a canonical Tornado Cash withdrawal; `ProvenWithdrawal` exposes
its call and encoded proof. See the method docs for errors and synchronous
execution.

Proving needs the circuit artifacts. The bindings do not load them: pass their
postcard-encoded bytes to `CircuitArtifacts.fromBytes`.

With generated bindings loaded and existing `pool`, `note`, `payer`, `recipient`,
`merkleProof` and artifact `bytes`:

```js
const artifacts = CircuitArtifacts.fromBytes(bytes);

const withdrawal = new Withdrawal(pool, note, recipient);
let proven;
try {
  withdrawal.withPayer(payer);
  proven = withdrawal.prove(merkleProof, artifacts);
  const call = proven.data();
  const proofBytes = proven.solidityProof();
} finally {
  proven?.free();
  withdrawal.free();
}
```
