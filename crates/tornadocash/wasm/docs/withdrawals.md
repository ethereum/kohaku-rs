# Withdrawal proving

`Withdrawal` proves a canonical Tornado Cash withdrawal; `ProvenWithdrawal` exposes
its call and encoded proof. See the method docs for consuming ownership, errors
and synchronous execution.

With generated bindings loaded and existing `pool`, `note`, `payer`, `recipient`
and `merkleProof`:

```js
const withdrawal = new Withdrawal(pool, note, recipient).withPayer(payer);
const proven = withdrawal.prove(merkleProof);
try {
  const call = proven.data();
  const proofBytes = proven.solidityProof();
} finally {
  proven.free();
}
```
