const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { createServer } = require('node:http');
const path = require('node:path');
const { before, after, test } = require('node:test');
const { chromium } = require('playwright');
const { vector, addresses, field, word, artifactBytes } = require('./fixtures.cjs');

const webDir = process.env.KOHAKU_TORNADO_WEB_BINDINGS || path.resolve(
  __dirname, '../../../../target/tornadocash-wasm-release-web',
);
let http, browser, page, origin;
const requests = [];
const fixtureErrors = [];

before(async () => {
  const artifacts = artifactBytes();
  const routes = new Map([
    ['/wasm.js', ['text/javascript', readFileSync(path.join(webDir, 'kohaku_tornadocash_wasm.js'))]],
    ['/kohaku_tornadocash_wasm_bg.wasm', ['application/wasm', readFileSync(path.join(webDir, 'kohaku_tornadocash_wasm_bg.wasm'))]],
    ['/artifacts', ['application/octet-stream', artifacts]],
    ['/cache/1_ETH_0.1_deposits.ndjson', ['application/x-ndjson',
      `{"block_number":9007199254740993,"commitment":"${field(1)}","leaf_index":0}\n`]],
    ['/cache/1_ETH_0.1_nullifiers.ndjson', ['application/x-ndjson', '']],
    ['/', ['text/html', '<!doctype html><title>Tornado WASM integration tests</title>']],
    ['/worker.js', ['text/javascript', `
      import init, { Withdrawal } from '/wasm.js';
      const ready = init();
      self.onmessage = async ({ data }) => {
        await ready;
        const withdrawal = Withdrawal.fromJSON(data);
        try { self.postMessage(withdrawal.toJSON()); }
        finally { withdrawal.free(); }
        self.close();
      };
    `]],
  ]);
  http = createServer(async (req, res) => {
    try {
      if (req.url === '/favicon.ico') { res.writeHead(204); res.end(); return; }
      if (req.url === '/rpc') {
        let text = '';
        for await (const chunk of req) text += chunk;
        const rpc = JSON.parse(text);
        assert.equal(rpc.method, 'eth_call');
        assert.equal(rpc.params[0].to, addresses.pool);
        assert.equal(rpc.params[0].input ?? rpc.params[0].data, `0xe5285dcc${word(1)}`);
        requests.push(rpc.method);
        res.setHeader('content-type', 'application/json');
        res.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, result: `0x${word(1)}` }));
        return;
      }
      const entry = routes.get(req.url);
      if (!entry) { res.writeHead(404); res.end(); return; }
      res.setHeader('content-type', entry[0]);
      res.end(entry[1]);
    } catch (error) {
      fixtureErrors.push(error);
      res.writeHead(500); res.end('unexpected fixture request');
    }
  });
  await new Promise((resolve, reject) => {
    http.once('error', reject); http.listen(0, '127.0.0.1', resolve);
  });
  origin = `http://127.0.0.1:${http.address().port}`;
  browser = await chromium.launch({ headless: true, executablePath: process.env.KOHAKU_CHROMIUM_EXECUTABLE });
  page = await browser.newPage();
  await page.goto(origin);
  await page.evaluate(async () => {
    const api = await import('/wasm.js');
    await api.default();
    globalThis.api = api;
  });
  console.log(`Browser runtime: ${browser.version()}`);
});

after(async () => {
  await browser?.close();
  if (http) await new Promise(resolve => { http.close(resolve); http.closeAllConnections(); });
  assert.deepEqual(fixtureErrors, []);
});

test('web bindings compose notes and deposits without consuming the note or pool', async () => {
  const result = await page.evaluate(({ vector, addresses }) => {
    const { Asset, Pool, Note, NoteString, Deposit } = globalThis.api;
    const asset = Asset.eth();
    const pool = new Pool(1n, addresses.pool, asset, 100000000000000000n, 0n);
    const note = new Note(vector.nullifier, vector.secret);
    const text = new NoteString(note, 'eth', '0.1', 1n);
    const deposit = new Deposit(pool, note);
    try {
      return { text: text.toString(), hash: note.nullifierHash(), commitment: note.commitment(),
        call: deposit.depositData(), approval: deposit.approvalData(), noteSecret: note.secret, poolAddress: pool.address };
    } finally { deposit.free(); text.free(); note.free(); pool.free(); asset.free(); }
  }, { vector, addresses });
  assert.equal(result.hash, vector.nullifierHash);
  assert.equal(result.text, `tornado-eth-0.1-1-${vector.nullifier}${vector.secret.slice(2)}`);
  assert.deepEqual(result.call, { to: addresses.pool, data: `0xb214faa5${result.commitment.slice(2)}`, value: 100000000000000000n });
  assert.equal(result.approval, undefined);
  assert.equal(result.noteSecret, vector.secret);
  assert.equal(result.poolAddress, addresses.pool);
});

test('web Merkle insertion retains proofs and rejects a malformed batch atomically', async () => {
  const result = await page.evaluate(({ leaves, invalid }) => {
    const tree = globalThis.api.MerkleTree.fromLeaves(leaves.slice(0, 1));
    const captured = tree.proof(0);
    const before = tree.root();
    let rejected = false;
    try {
      try { tree.splice(0, [leaves[1], invalid]); } catch { rejected = true; }
      const afterFailure = tree.root();
      tree.splice(1, [leaves[1]]);
      const appended = tree.leafProof(leaves[1]);
      try { return { rejected, before, afterFailure, root: tree.root(), capturedRoot: captured.root,
        capturedValid: captured.verify(), appendedLeaf: appended.leaf, appendedValid: appended.verify() }; }
      finally { appended.free(); }
    } finally { captured.free(); tree.free(); }
  }, { leaves: [field(1), field(2)], invalid: `0x${'ff'.repeat(32)}` });
  assert.equal(result.rejected, true);
  assert.equal(result.afterFailure, result.before);
  assert.equal(result.capturedRoot, result.before);
  assert.notEqual(result.root, result.before);
  assert.equal(result.capturedValid, true);
  assert.equal(result.appendedValid, true);
  assert.equal(result.appendedLeaf, field(2));
});

test('web proving uses runtime artifacts and preserves withdrawal JSON and call values', { timeout: 120000 }, async () => {
  const result = await page.evaluate(async ({ vector, addresses }) => {
    const api = globalThis.api;
    const bytes = new Uint8Array(await (await fetch('/artifacts')).arrayBuffer());
    const artifacts = api.CircuitArtifacts.fromBytes(bytes);
    const asset = api.Asset.erc20(addresses.token, 'TKN', 18);
    const pool = new api.Pool(1n, addresses.pool, asset, 100000000000000000001n, 0n);
    const note = new api.Note(vector.nullifier, vector.secret);
    const tree = api.MerkleTree.fromLeaves([note.commitment()]);
    const proof = tree.proof(0);
    const withdrawal = new api.Withdrawal(pool, note, addresses.recipient);
    const payer = new api.Payer(addresses.payer, 3n, 42n);
    let restored, proven, recovered;
    try {
      withdrawal.withPayer(payer);
      const json = withdrawal.toJSON();
      restored = api.Withdrawal.fromJSON(json);
      proven = restored.prove(proof, artifacts);
      recovered = api.ProvenWithdrawal.fromJSON(proven.toJSON());
      return { json, after: restored.toJSON(), call: proven.data(), recoveredCall: recovered.data(), proof: recovered.solidityProof() };
    } finally {
      recovered?.free(); proven?.free(); restored?.free(); payer.free(); withdrawal.free();
      proof.free(); tree.free(); note.free(); pool.free(); asset.free(); artifacts.free();
    }
  }, { vector, addresses });
  assert.equal(result.after, result.json);
  assert.match(result.json, /100000000000000000001/);
  assert.deepEqual(result.call, result.recoveredCall);
  assert.equal(result.call.to, addresses.pool);
  assert.equal(result.call.value, 42n);
  assert.equal(result.call.data.slice(0, 10), '0x21a0adb6');
  assert.match(result.proof, /^0x[0-9a-f]{512}$/);
});

test('web provider uses fetch with the correct encoded request and exposes the result', async () => {
  const result = await page.evaluate(async ({ addresses, hash }) => {
    const asset = globalThis.api.Asset.eth();
    const pool = new globalThis.api.Pool(1n, addresses.pool, asset, 1n, 0n);
    const provider = new globalThis.api.TornadoProvider(`${location.origin}/rpc`);
    try { return await provider.isSpent(pool, hash); }
    finally { provider.free(); pool.free(); asset.free(); }
  }, { addresses, hash: field(1) });
  assert.equal(result, true);
  assert.deepEqual(requests, ['eth_call']);
});

test('web remote synchronization retains bigint blocks and exposes usable snapshot events', async () => {
  const result = await page.evaluate(async ({ addresses }) => {
    const api = globalThis.api;
    const asset = api.Asset.eth();
    const pool = new api.Pool(1n, addresses.pool, asset, 100000000000000000n, 0n);
    const remote = new api.RemoteSyncer(`${location.origin}/cache`);
    const tree = new api.MerkleTree();
    let snapshot, proof;
    try {
      snapshot = await remote.syncRange(pool, 9007199254740993n, 9007199254740999n);
      tree.spliceEvents(snapshot);
      proof = tree.proof(0);
      return { from: snapshot.fromBlock, to: snapshot.toBlock, events: snapshot.events,
        leaf: proof.leaf, valid: proof.verify() };
    } finally { proof?.free(); snapshot?.free(); tree.free(); remote.free(); pool.free(); asset.free(); }
  }, { addresses });
  assert.equal(result.from, 9007199254740993n);
  assert.equal(result.to, 9007199254740994n);
  assert.deepEqual(result.events, [{ kind: 'deposit', commitment: field(1), leafIndex: 0, blockNumber: 9007199254740993n }]);
  assert.equal(result.leaf, field(1));
  assert.equal(result.valid, true);
});

test('web event decoding and integer validation retain their original JS input types', async () => {
  const result = await page.evaluate(({ hash, topic, data, addresses }) => {
    const api = globalThis.api;
    const event = api.decodeEvent({ topics: [topic, hash], data, blockNumber: 9007199254740993n });
    const asset = api.Asset.eth();
    const rejected = [];
    try {
      for (const amount of [1, -1n, 1n << 128n]) {
        try {
          const unexpected = new api.Pool(1n, addresses.pool, asset, amount, 0n);
          unexpected.free(); rejected.push(false);
        } catch { rejected.push(true); }
      }
      return { event, rejected, absent: api.decodeEvent({ topics: [], data: '0x' }) };
    } finally { asset.free(); }
  }, { hash: field(1), topic: '0xa945e51eec50ab98c161376f0db4cf2aeba3ec92755fe2fcd388bdbbb80ff196',
    data: `0x${word(7)}${word(123)}`, addresses });
  assert.deepEqual(result.event, { kind: 'deposit', commitment: field(1), leafIndex: 7, blockNumber: 9007199254740993n });
  assert.deepEqual(result.rejected, [true, true, true]);
  assert.equal(result.absent, undefined);
});

test('withdrawal JSON crosses a browser module worker without rounding a large pool amount', { timeout: 15000 }, async () => {
  const result = await page.evaluate(async ({ vector, addresses }) => {
    const api = globalThis.api;
    const asset = api.Asset.eth();
    const pool = new api.Pool(1n, addresses.pool, asset, 100000000000000000001n, 0n);
    const note = new api.Note(vector.nullifier, vector.secret);
    const withdrawal = new api.Withdrawal(pool, note, addresses.recipient);
    const worker = new Worker('/worker.js', { type: 'module' });
    try {
      const sent = withdrawal.toJSON();
      const returned = await new Promise((resolve, reject) => {
        worker.onmessage = event => resolve(event.data);
        worker.onerror = event => reject(new Error(event.message));
        worker.postMessage(sent);
      });
      return { sent, returned };
    } finally { worker.terminate(); withdrawal.free(); note.free(); pool.free(); asset.free(); }
  }, { vector, addresses });
  assert.equal(result.returned, result.sent);
  assert.match(result.returned, /100000000000000000001/);
});
