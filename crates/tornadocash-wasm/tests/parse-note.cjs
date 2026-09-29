const assert = require('node:assert/strict');
const { resolve } = require('node:path');
const { test } = require('node:test');

if (!process.env.TORNADOCASH_WASM_MODULE) {
  throw new Error('Set TORNADOCASH_WASM_MODULE to the generated wasm-bindgen JavaScript module');
}
const { Note } = require(resolve(process.env.TORNADOCASH_WASM_MODULE));
const nullifier = Uint8Array.from({ length: 31 }, (_, i) => i);
const secret = Uint8Array.from({ length: 31 }, (_, i) => i + 31);
const preimage = Buffer.concat([nullifier, secret]).toString('hex');

test('Note.parse returns an instance whose toObject returns exact metadata and ordered secret bytes', (t) => {
  const instance = Note.parse(`tornado-eth-0.10-1-0x${preimage}`);

  t.after(() => instance.free());
  assert.ok(instance instanceof Note);
  const note = instance.toObject();

  assert.equal(Object.getPrototypeOf(note), Object.prototype);
  assert.deepEqual(note, {
    symbol: 'eth',
    amount: '0.10',
    chainId: 1n,
    nullifier,
    secret,
  });
  assert.ok(note.nullifier instanceof Uint8Array);
  assert.ok(note.secret instanceof Uint8Array);
});

test('Note.parse preserves chain IDs as bigint across the u64 range', (t) => {
  for (const chainId of [0n, 9007199254740993n, 18446744073709551615n]) {
    const instance = Note.parse(`tornado-eth-1-${chainId}-0x${preimage}`);
    t.after(() => instance.free());
    const note = instance.toObject();
    assert.equal(note.chainId, chainId);
  }
});

test('Note.parse keeps core parsing behavior for optional hex prefix and metadata', (t) => {
  const instance = Note.parse(`tornado-TOKEN-custom-1-${preimage}`);
  t.after(() => instance.free());
  const note = instance.toObject();
  assert.equal(note.symbol, 'TOKEN');
  assert.equal(note.amount, 'custom');
  assert.deepEqual(note.nullifier, nullifier);
  assert.deepEqual(note.secret, secret);
});

test('Note.parse exposes core parsing failures as JavaScript Errors', () => {
  const cases = [
    ['empty text', '', 'invalid note format'],
    ['leading whitespace', ` tornado-eth-1-1-0x${preimage}`, 'invalid note format'],
    ['invalid chainId', `tornado-eth-1-invalid-0x${preimage}`, 'invalid chain id'],
    ['chainId above u64', `tornado-eth-1-18446744073709551616-0x${preimage}`, 'invalid chain id'],
    ['short preimage', 'tornado-eth-1-1-0x00', 'invalid note format'],
    ['long preimage', `tornado-eth-1-1-0x${'ff'.repeat(63)}`, 'invalid note format'],
    ['invalid hex', `tornado-eth-1-1-0x${'zz'.repeat(62)}`, /invalid hex/],
  ];
  for (const [name, note, message] of cases) {
    assert.throws(() => Note.parse(note), { name: 'Error', message }, name);
  }
});
