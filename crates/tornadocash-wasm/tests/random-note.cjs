const assert = require('node:assert/strict');
const { resolve } = require('node:path');
const { test } = require('node:test');

if (!process.env.TORNADOCASH_WASM_MODULE) {
  throw new Error('Set TORNADOCASH_WASM_MODULE to the generated wasm-bindgen JavaScript module');
}
const { Note } = require(resolve(process.env.TORNADOCASH_WASM_MODULE));

test('Note.random creates fresh notes that round-trip through the core format', (t) => {
  const first = Note.random('eth', '0.10', 1n);
  t.after(() => first.free());
  const second = Note.random('eth', '0.10', 1n);
  t.after(() => second.free());
  assert.ok(first instanceof Note);
  const data = first.toObject();
  assert.equal(data.symbol, 'eth');
  assert.equal(data.amount, '0.10');
  assert.equal(data.chainId, 1n);
  assert.ok(data.nullifier instanceof Uint8Array);
  assert.ok(data.secret instanceof Uint8Array);
  assert.equal(data.nullifier.length, 31);
  assert.equal(data.secret.length, 31);
  assert.notDeepEqual(first.preimage(), second.preimage());

  const parsed = Note.parse(first.toString());
  t.after(() => parsed.free());
  assert.deepEqual(parsed.toObject(), data);
  assert.equal(parsed.commitment(), first.commitment());
  assert.equal(parsed.nullifierHash(), first.nullifierHash());
});

test('Note.random preserves metadata and chain IDs across the u64 range', (t) => {
  for (const chainId of [0n, 9007199254740993n, 18446744073709551615n]) {
    const note = Note.random('TOKEN', 'custom', chainId);
    t.after(() => note.free());
    const data = note.toObject();
    assert.equal(data.chainId, chainId);
    assert.equal(data.symbol, 'TOKEN');
    assert.equal(data.amount, 'custom');
  }
});

test('Note.random rejects invalid chain IDs instead of truncating them', () => {
  for (const chainId of [-1n, 18446744073709551616n, 1, 1.5, '1', null, undefined]) {
    assert.throws(() => Note.random('eth', '1', chainId), {
      name: 'Error',
      message: 'chainId must be a bigint in the u64 range',
    }, `chainId ${typeof chainId}: ${String(chainId)}`);
  }
});

test('Note.random throws if Web Crypto is absent or fails, and recovers afterward', (t) => {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'crypto');
  try {
    const cases = [
      ['missing Web Crypto', undefined],
      ['failing Web Crypto', { getRandomValues() { throw new Error('unavailable'); } }],
    ];
    for (const [name, crypto] of cases) {
      Object.defineProperty(globalThis, 'crypto', { configurable: true, value: crypto });
      assert.throws(() => Note.random('eth', '1', 1n), Error, name);
    }
  } finally {
    Object.defineProperty(globalThis, 'crypto', original);
  }
  const note = Note.random('eth', '1', 1n);
  t.after(() => note.free());
  assert.equal(note.preimage().length, 62);
});
