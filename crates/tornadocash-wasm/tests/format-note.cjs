const assert = require('node:assert/strict');
const { resolve } = require('node:path');
const { test } = require('node:test');

if (!process.env.TORNADOCASH_WASM_MODULE) {
  throw new Error('Set TORNADOCASH_WASM_MODULE to the generated wasm-bindgen JavaScript module');
}
const { Note } = require(resolve(process.env.TORNADOCASH_WASM_MODULE));
const fields = {
  symbol: 'eth',
  amount: '0.10',
  chainId: 1n,
  nullifier: Uint8Array.from({ length: 31 }, (_, i) => i),
  secret: Uint8Array.from({ length: 31 }, (_, i) => 255 - i),
};
const preimage = Buffer.concat([fields.nullifier, fields.secret]).toString('hex');

test('Note constructor and toString produce the core legacy format', (t) => {
  const note = new Note(fields);
  t.after(() => note.free());
  const text = note.toString();
  assert.equal(text, `tornado-eth-0.10-1-0x${preimage}`);
  const parsed = Note.parse(text);
  t.after(() => parsed.free());
  assert.deepEqual(parsed.toObject(), fields);
  assert.equal(parsed.commitment(), note.commitment());
  assert.equal(parsed.nullifierHash(), note.nullifierHash());
});

test('Note.toString canonicalizes hex while preserving the amount text', (t) => {
  const note = Note.parse(`tornado-eth-0.10-1-${preimage.toUpperCase()}`);
  t.after(() => note.free());
  assert.equal(note.toString(), `tornado-eth-0.10-1-0x${preimage}`);
});

test('Note constructor preserves chain IDs across the u64 range', (t) => {
  for (const chainId of [0n, 9007199254740993n, 18446744073709551615n]) {
    const data = { ...fields, chainId };
    const note = new Note(data);
    t.after(() => note.free());
    assert.equal(note.toString(), `tornado-eth-0.10-${chainId}-0x${preimage}`);
    const parsed = Note.parse(note.toString());
    t.after(() => parsed.free());
    assert.deepEqual(parsed.toObject(), data);
  }
});

test('Note retains the core constructor and formatter metadata behavior', (t) => {
  const note = new Note({ ...fields, symbol: 'TOKEN', amount: 'custom' });
  t.after(() => note.free());
  assert.equal(note.toString(), `tornado-TOKEN-custom-1-0x${preimage}`);
});

test('Note constructor rejects invalid structured inputs with JavaScript Errors', (t) => {
  const { secret, ...missingSecret } = fields;
  const invalid = [
    ['undefined input', undefined],
    ['null input', null],
    ['string input', 'not an object'],
    ['missing secret', missingSecret],
    ['numeric symbol', { ...fields, symbol: 1 }],
    ['numeric amount', { ...fields, amount: 0.1 }],
    ['negative chainId', { ...fields, chainId: -1n }],
    ['chainId above u64', { ...fields, chainId: 18446744073709551616n }],
    ['fractional chainId', { ...fields, chainId: 1.5 }],
    ...[1, 9007199254740992, '1', null, undefined].map(chainId =>
      [`chainId ${typeof chainId}: ${String(chainId)}`, { ...fields, chainId }]),
    ...['nullifier', 'secret'].flatMap(field =>
      [
        ['30 bytes', new Uint8Array(30)],
        ['32 bytes', new Uint8Array(32)],
        ['plain array', Array(31).fill(0)],
        ['ArrayBuffer', new ArrayBuffer(31)],
        ['DataView', new DataView(new ArrayBuffer(31))],
        ['Uint16Array', new Uint16Array(31)],
      ].map(([name, value]) => [`${field}: ${name}`, { ...fields, [field]: value }])),
  ];
  for (const [name, data] of invalid) {
    assert.throws(() => new Note(data), Error, name);
  }
  // A failed conversion must leave the WASM module usable.
  const note = new Note(fields);
  t.after(() => note.free());
  assert.equal(note.toString(), `tornado-eth-0.10-1-0x${preimage}`);
});

test('Note constructor copies only the selected Uint8Array view', (t) => {
  const buffer = new Uint8Array(100).fill(99);
  buffer.set(fields.nullifier, 7);
  buffer.set(fields.secret, 50);
  const note = new Note({
    ...fields,
    nullifier: buffer.subarray(7, 38),
    secret: buffer.subarray(50, 81),
  });
  t.after(() => note.free());
  buffer.fill(0);
  assert.deepEqual(note.toObject(), fields);
});

test('Note owns its data and toObject returns independent snapshots', (t) => {
  const input = { ...fields, nullifier: fields.nullifier.slice(), secret: fields.secret.slice() };
  const note = new Note(input);
  t.after(() => note.free());
  const commitment = note.commitment();
  const nullifierHash = note.nullifierHash();
  input.symbol = 'changed';
  input.nullifier.fill(0);
  input.secret.fill(0);
  const snapshot = note.toObject();
  snapshot.amount = 'changed';
  snapshot.nullifier.fill(255);
  snapshot.secret.fill(255);
  assert.deepEqual(note.toObject(), fields);
  assert.equal(note.toString(), `tornado-eth-0.10-1-0x${preimage}`);
  assert.equal(note.commitment(), commitment);
  assert.equal(note.nullifierHash(), nullifierHash);
});
