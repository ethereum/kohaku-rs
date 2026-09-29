const assert = require('node:assert/strict');
const { resolve } = require('node:path');
const { test } = require('node:test');

if (!process.env.TORNADOCASH_WASM_MODULE) {
  throw new Error('Set TORNADOCASH_WASM_MODULE to the generated wasm-bindgen JavaScript module');
}
const { Note } = require(resolve(process.env.TORNADOCASH_WASM_MODULE));
const preimage = '01'.repeat(31) + '02'.repeat(31);
const { vectors } = require('./fixtures/note-vectors.json');

for (const vector of vectors) {
  test(`Note hashes match circomlibjs: ${vector.name}`, (t) => {
    const constructed = new Note({
      symbol: 'eth', amount: '1', chainId: 1n,
      nullifier: Uint8Array.from(Buffer.from(vector.nullifier, 'hex')),
      secret: Uint8Array.from(Buffer.from(vector.secret, 'hex')),
    });
    t.after(() => constructed.free());
    const parsed = Note.parse(`tornado-eth-1-1-0x${vector.nullifier}${vector.secret}`);
    t.after(() => parsed.free());
    for (const note of [constructed, parsed]) {
      assert.equal(note.commitment(), vector.commitment);
      assert.equal(note.nullifierHash(), vector.nullifierHash);
    }
  });
}

test('Note.preimage returns an independent Uint8Array in core byte order', (t) => {
  const expected = Uint8Array.from({ length: 62 }, (_, i) => (i * 17) % 256);
  const text = `tornado-eth-1-1-0x${Buffer.from(expected).toString('hex')}`;
  const note = Note.parse(text);
  t.after(() => note.free());

  const bytes = note.preimage();
  assert.ok(bytes instanceof Uint8Array);
  assert.equal(bytes.length, 62);
  assert.deepEqual(bytes, expected);
  const data = note.toObject();
  assert.deepEqual(bytes.slice(0, 31), data.nullifier);
  assert.deepEqual(bytes.slice(31), data.secret);

  bytes.fill(0);
  assert.deepEqual(note.preimage(), expected);
  assert.equal(note.toString(), text);
});

for (const method of ['commitment', 'nullifierHash']) {
  test(`Note.${method} returns a 32-byte lowercase hex string`, (t) => {
    const note = Note.parse(`tornado-eth-1-1-0x${preimage}`);
    t.after(() => note.free());
    const hash = note[method]();
    assert.equal(typeof hash, 'string');
    assert.match(hash, /^0x[0-9a-f]{64}$/);
    assert.equal(note[method](), hash);
  });
}

test('nullifier hash depends on the nullifier, independently of the secret and metadata', (t) => {
  const notes = [
    Note.parse(`tornado-eth-1-1-0x${preimage}`),
    Note.parse(`tornado-eth-1-1-0x${'01'.repeat(31)}${'03'.repeat(31)}`),
    Note.parse(`tornado-dai-100-5-0x${preimage}`),
    Note.parse(`tornado-eth-1-1-0x${'03'.repeat(31)}${'02'.repeat(31)}`),
  ];
  for (const note of notes) t.after(() => note.free());
  const hash = notes[0].nullifierHash();
  assert.equal(hash, notes[1].nullifierHash());
  assert.equal(hash, notes[2].nullifierHash());
  assert.notEqual(hash, notes[3].nullifierHash());
});

test('two notes coexist in one WASM module and can be freed independently', (t) => {
  const first = Note.parse(`tornado-eth-1-1-0x${preimage}`);
  const second = Note.parse(`tornado-eth-1-1-0x${'03'.repeat(62)}`);
  t.after(() => second.free());
  const firstHash = first.commitment();
  const secondHash = second.commitment();
  const snapshot = first.toObject();
  const bytes = first.preimage();
  assert.notEqual(firstHash, secondHash);
  first.free();
  for (const method of ['toObject', 'toString', 'preimage', 'commitment', 'nullifierHash']) {
    assert.throws(() => first[method](), Error);
  }
  assert.equal(second.commitment(), secondHash);
  assert.equal(Buffer.from(bytes).toString('hex'), preimage);
  assert.deepEqual(snapshot.nullifier, new Uint8Array(31).fill(1));
  assert.deepEqual(snapshot.secret, new Uint8Array(31).fill(2));
});
