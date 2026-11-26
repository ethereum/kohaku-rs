const assert = require('node:assert/strict');
const { test } = require('node:test');
const { Worker } = require('node:worker_threads');
const { once } = require('node:events');
const { wasm, bindingsPath, vector, addresses, word, field, ownership, pool, artifactBytes } = require('./helpers.cjs');

test('runtime artifacts prove a withdrawal and preserve its ABI fields through JSON reconstruction', t => {
  const own = ownership(t);
  const artifacts = own(wasm.CircuitArtifacts.fromBytes(artifactBytes()));
  const instance = pool(own, { erc20: true, amount: 100000000000000000001n });
  const note = own(new wasm.Note(vector.nullifier, vector.secret));
  const tree = own(wasm.MerkleTree.fromLeaves([note.commitment()]));
  const proof = own(tree.proof(0));
  const withdrawal = own(new wasm.Withdrawal(instance, note, addresses.recipient));
  const payer = own(new wasm.Payer(addresses.payer, 3n, 42n));
  assert.equal(withdrawal.withPayer(payer), undefined);
  const json = withdrawal.toJSON();
  assert.equal(typeof json, 'string');
  const restored = own(wasm.Withdrawal.fromJSON(json));
  assert.equal(restored.toJSON(), json);
  assert.match(json, /100000000000000000001/, 'amount must not be rounded through a JS number');
  const proven = own(restored.prove(proof, artifacts));
  const restoredProof = own(wasm.ProvenWithdrawal.fromJSON(proven.toJSON()));
  assert.deepEqual(restoredProof.data(), proven.data());
  assert.equal(restoredProof.solidityProof(), proven.solidityProof());
  assert.match(proven.solidityProof(), /^0x[0-9a-f]{512}$/);
  const call = proven.data();
  assert.equal(call.to, addresses.pool);
  assert.equal(call.value, 42n);
  assert.equal(call.data.slice(0, 10), '0x21a0adb6');
  const header = call.data.slice(10, 10 + 7 * 64);
  assert.equal(header, [word(224), proof.root.slice(2), vector.nullifierHash.slice(2), word(0x33), word(0x44), word(3), word(42)].join(''));
  assert.equal(call.data.slice(10 + 7 * 64), `${word(256)}${proven.solidityProof().slice(2)}`);
  assert.equal(restored.toJSON(), json, 'prove must not consume its withdrawal');
  assert.equal(proof.verify(), true);
  assert.equal(note.nullifierHash(), vector.nullifierHash);
});

test('withdrawal input failures reject invalid artifacts, JSON and proof associations', t => {
  const own = ownership(t);
  assert.throws(() => wasm.CircuitArtifacts.fromBytes(new Uint8Array([1, 2, 3])));
  assert.throws(() => wasm.Withdrawal.fromJSON('{}'));
  assert.throws(() => wasm.ProvenWithdrawal.fromJSON('{}'));
  const artifacts = own(wasm.CircuitArtifacts.fromBytes(artifactBytes()));
  const instance = pool(own);
  const note = own(new wasm.Note(vector.nullifier, vector.secret));
  assert.throws(() => new wasm.Withdrawal(instance, note, '0x01'));
  const withdrawal = own(new wasm.Withdrawal(instance, note, addresses.recipient));
  const otherTree = own(wasm.MerkleTree.fromLeaves([field(1)]));
  const otherProof = own(otherTree.proof(0));
  assert.throws(() => withdrawal.prove(otherProof, artifacts), /leaf mismatch/i);
  const tree = own(wasm.MerkleTree.fromLeaves([note.commitment()]));
  const proof = own(tree.proof(0));
  const payer = own(new wasm.Payer(addresses.payer, 0n, 1n));
  withdrawal.withPayer(payer);
  assert.throws(() => withdrawal.prove(proof, artifacts), /refund/i);
  assert.equal(proof.verify(), true);
  assert.equal(note.secret, vector.secret);
});

test('same-version withdrawal JSON crosses a worker and returns a real proven withdrawal', { timeout: 120000 }, async t => {
  const own = ownership(t);
  const instance = pool(own, { erc20: true, amount: 100000000000000000001n });
  const note = own(new wasm.Note(vector.nullifier, vector.secret));
  const withdrawal = own(new wasm.Withdrawal(instance, note, addresses.recipient));
  assert.match(withdrawal.toJSON(), /100000000000000000001/);
  const worker = new Worker(`
    const { parentPort, workerData } = require('node:worker_threads');
    const api = require(workerData.bindingsPath);
    const withdrawal = api.Withdrawal.fromJSON(workerData.json);
    const artifacts = api.CircuitArtifacts.fromBytes(workerData.bytes);
    const tree = api.MerkleTree.fromLeaves(workerData.leaves);
    const proof = tree.proof(0);
    let proven;
    try {
      proven = withdrawal.prove(proof, artifacts);
      parentPort.postMessage({ withdrawal: withdrawal.toJSON(), proven: proven.toJSON() });
    } finally {
      proven?.free(); proof.free(); tree.free(); artifacts.free(); withdrawal.free();
    }
  `, {
    eval: true,
    workerData: { bindingsPath, json: withdrawal.toJSON(), bytes: artifactBytes(), leaves: [note.commitment()] },
  });
  t.after(() => worker.terminate());
  const [[message], [code]] = await Promise.all([once(worker, 'message'), once(worker, 'exit')]);
  assert.equal(message.withdrawal, withdrawal.toJSON());
  assert.match(message.withdrawal, /100000000000000000001/);
  const proven = own(wasm.ProvenWithdrawal.fromJSON(message.proven));
  assert.equal(proven.data().to, addresses.pool);
  assert.equal(proven.data().value, 0n);
  assert.match(proven.solidityProof(), /^0x[0-9a-f]{512}$/);
  assert.equal(code, 0);
});
