const assert = require('node:assert/strict');
const { test } = require('node:test');
const { wasm, vector, addresses, word, field, ownership, pool } = require('./helpers.cjs');

test('a portable note preserves secrets and matches the circuit nullifier-hash vector', t => {
  const own = ownership(t);
  const note = own(new wasm.Note(vector.nullifier, vector.secret));
  const portable = own(new wasm.NoteString(note, 'eth', '0.1', 1n));
  const text = `tornado-eth-0.1-1-${vector.nullifier}${vector.secret.slice(2)}`;
  assert.equal(portable.toString(), text);
  const parsed = own(wasm.NoteString.parse(text));
  assert.equal(parsed.preimage(), `${vector.nullifier}${vector.secret.slice(2)}`);
  assert.equal(parsed.nullifier, vector.nullifier);
  assert.equal(parsed.secret, vector.secret);
  assert.equal(parsed.chainId, 1n);
  assert.equal(parsed.nullifierHash(), vector.nullifierHash);
  assert.equal(note.nullifierHash(), vector.nullifierHash);
  const known = own(wasm.Pool.fromNote(parsed));
  assert.equal(known.address, '0x12D66f87A04A9E220743712cE6d9bB1B5616B8Fc');
  assert.equal(known.amountWei, 100000000000000000n);
  assert.equal(parsed.toString(), text, 'pool lookup must not consume the note');
});

test('generated constructors reject malformed note and integer inputs without coercion', t => {
  const own = ownership(t);
  const asset = own(wasm.Asset.eth());
  assert.throws(() => new wasm.Note('0x01', vector.secret));
  assert.throws(() => wasm.NoteString.parse('tornado-eth-0.1-1-0x01'));
  for (const decimals of [-1, 256, 1.5, NaN, Infinity, 18n, '18']) {
    assert.throws(() => wasm.Asset.native('ETH', decimals));
  }
  for (const chainId of [-1n, 1n << 64n, 1, '1', null]) {
    assert.throws(() => new wasm.Pool(chainId, addresses.pool, asset, 1n, 0n));
  }
  for (const amount of [-1n, 1n << 128n, 1, '1']) {
    assert.throws(() => new wasm.Pool(1n, addresses.pool, asset, amount, 0n));
  }
});

test('custom pool and deposit preserve large amounts and independently owned asset metadata', t => {
  const own = ownership(t);
  const asset = wasm.Asset.erc20(addresses.token, 'TKN', 18);
  const amount = (1n << 100n) + 7n;
  let custom;
  try {
    custom = own(new wasm.Pool(9007199254740993n, addresses.pool, asset, amount, 9007199254740995n));
  } finally {
    asset.free();
  }
  const metadata = own(custom.asset);
  assert.equal(metadata.address, addresses.token);
  assert.equal(custom.chainId, 9007199254740993n);
  assert.equal(custom.deployedBlock, 9007199254740995n);
  assert.equal(custom.amountWei, amount);
  assert.equal(custom.canonical, false);
  const note = own(new wasm.Note(vector.nullifier, vector.secret));
  const deposit = own(new wasm.Deposit(custom, note));
  assert.deepEqual(deposit.approvalData(), {
    to: addresses.token,
    data: `0x095ea7b3${word(0x11)}${word(amount)}`,
    value: 0n,
  });
  assert.equal(custom.amountWei, amount);
  assert.equal(note.secret, vector.secret);
});

for (const erc20 of [false, true]) {
  test(`${erc20 ? 'ERC20' : 'native'} deposit encodes the note and correct call value without consuming inputs`, t => {
    const own = ownership(t);
    const instance = pool(own, { erc20 });
    const note = own(new wasm.Note(vector.nullifier, vector.secret));
    const commitment = note.commitment();
    const deposit = own(new wasm.Deposit(instance, note));
    assert.deepEqual(deposit.depositData(), {
      to: addresses.pool,
      data: `0xb214faa5${commitment.slice(2)}`,
      value: erc20 ? 0n : 100000000000000000n,
    });
    if (!erc20) assert.equal(deposit.approvalData(), undefined);
    else assert.deepEqual(deposit.approvalData(), {
      to: addresses.token,
      data: `0x095ea7b3${word(0x11)}${word(100000000000000000n)}`,
      value: 0n,
    });
    assert.equal(note.commitment(), commitment);
    assert.equal(instance.address, addresses.pool);
  });
}

test('Merkle append and replacement agree with ordered construction and retain captured proofs', t => {
  const own = ownership(t);
  const tree = own(wasm.MerkleTree.fromLeaves([field(1), field(2)]));
  const captured = own(tree.proof(0));
  const originalRoot = captured.root;
  tree.splice(1, [field(3), field(4)]);
  const expected = own(wasm.MerkleTree.fromLeaves([field(1), field(3), field(4)]));
  assert.equal(tree.root(), expected.root());
  assert.notEqual(tree.root(), originalRoot);
  assert.equal(captured.root, originalRoot);
  assert.equal(captured.verify(), true);
  const proof = own(tree.leafProof(field(4)));
  assert.equal(proof.leaf, field(4));
  assert.equal(proof.root, tree.root());
  assert.equal(proof.verify(), true);
  tree.splice(999, []);
  assert.equal(tree.root(), expected.root(), 'empty out-of-range insertion is documented as a no-op');
});

test('invalid Merkle batches and indices leave a previously valid tree usable', t => {
  const own = ownership(t);
  const tree = own(wasm.MerkleTree.fromLeaves([field(1)]));
  const before = tree.root();
  assert.throws(() => tree.splice(0, [field(2), `0x${'ff'.repeat(32)}`]));
  assert.equal(tree.root(), before);
  for (const index of [-1, 0.5, NaN, Infinity, 2 ** 32, 0n, '0']) {
    assert.throws(() => tree.proof(index));
    assert.throws(() => tree.splice(index, [field(2)]));
  }
  assert.throws(() => tree.proof(1));
  assert.throws(() => tree.leafProof(field(2)));
  assert.throws(() => tree.splice(2, [field(2)]));
  assert.equal(tree.root(), before);
  assert.equal(own(tree.proof(0)).verify(), true);
});

test('runtime randomness produces usable 31-byte note secrets', t => {
  const own = ownership(t);
  const note = own(wasm.Note.random());
  assert.match(note.nullifier, /^0x[0-9a-f]{62}$/);
  assert.match(note.secret, /^0x[0-9a-f]{62}$/);
  const portable = own(new wasm.NoteString(note, 'eth', '0.1', 1n));
  assert.equal(own(wasm.NoteString.parse(portable.toString())).preimage(), note.preimage());
});
