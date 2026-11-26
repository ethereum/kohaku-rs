const assert = require('node:assert/strict');
const { test } = require('node:test');
const { wasm, addresses, field, word, ownership, pool, server, jsonBody } = require('./helpers.cjs');

test('provider queries encode their pool and hashes and preserve spent-array ordering', async t => {
  const own = ownership(t);
  const instance = pool(own);
  const calls = [];
  const expected = [
    [`0xe5285dcc${word(1)}`, `0x${word(1)}`],
    [`0x6d9833e3${word(2)}`, `0x${word(0)}`],
    [`0x9fa12d0b${word(32)}${word(2)}${word(1)}${word(2)}`, `0x${word(32)}${word(2)}${word(1)}${word(0)}`],
  ];
  const url = await server(t, async (req, res) => {
    assert.equal(req.method, 'POST');
    const rpc = await jsonBody(req);
    assert.equal(rpc.method, 'eth_call');
    assert.equal(rpc.params[0].to, addresses.pool);
    const [data, result] = expected[calls.length];
    assert.equal(rpc.params[0].input ?? rpc.params[0].data, data);
    calls.push(data);
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, result }));
  });
  const provider = own(new wasm.TornadoProvider(url));
  assert.equal(await provider.isSpent(instance, field(1)), true);
  assert.equal(await provider.isKnownRoot(instance, field(2)), false);
  assert.deepEqual(await provider.isSpentArray(instance, [field(1), field(2)]), [true, false]);
  assert.equal(calls.length, 3);
  assert.equal(instance.address, addresses.pool);
});

test('provider rejects invalid fields locally and RPC errors without returning false', async t => {
  const own = ownership(t);
  const instance = pool(own);
  let calls = 0;
  const url = await server(t, async (req, res) => {
    calls++;
    const rpc = await jsonBody(req);
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, error: { code: -32000, message: 'fixture query rejected' } }));
  });
  const provider = own(new wasm.TornadoProvider(url));
  await assert.rejects(provider.isSpent(instance, '0x01'));
  await assert.rejects(provider.isSpentArray(instance, [field(1), `0x${'ff'.repeat(32)}`]));
  assert.equal(calls, 0);
  await assert.rejects(provider.isKnownRoot(instance, field(1)), /fixture query rejected/);
  assert.equal(calls, 1);
  assert.throws(() => new wasm.TornadoProvider('ws://127.0.0.1:8545'));
});

test('an in-flight provider query owns its handles independently of JS wrappers', async t => {
  const url = await server(t, async (req, res) => {
    const rpc = await jsonBody(req);
    assert.equal(rpc.params[0].to, addresses.pool);
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, result: `0x${word(1)}` }));
  });
  const asset = wasm.Asset.eth();
  const instance = new wasm.Pool(1n, addresses.pool, asset, 100000000000000000n, 0n);
  asset.free();
  const provider = new wasm.TornadoProvider(url);
  let pending;
  try {
    pending = provider.isSpent(instance, field(1));
  } finally {
    provider.free();
    instance.free();
  }
  assert.equal(await pending, true);
});

test('remote cache synchronization preserves exact block ranges and event data', async t => {
  const own = ownership(t);
  const instance = pool(own);
  const requested = [];
  const url = await server(t, (req, res) => {
    requested.push(req.url);
    res.setHeader('content-type', 'application/x-ndjson');
    if (req.url === '/1_ETH_0.1_deposits.ndjson') {
      res.end(`{"block_number":9007199254740993,"commitment":"${field(1)}","leaf_index":0}\n`);
    } else {
      assert.equal(req.url, '/1_ETH_0.1_nullifiers.ndjson');
      res.end(`{"block_number":9007199254740994,"nullifier":"${field(2)}","to":"${addresses.recipient}","fee":"0x10000000000000001"}\n`);
    }
  });
  const remote = own(new wasm.RemoteSyncer(url));
  const snapshot = own(await remote.syncRange(instance, 9007199254740993n, 9007199254741000n));
  assert.equal(snapshot.fromBlock, 9007199254740993n);
  assert.equal(snapshot.toBlock, 9007199254740995n);
  const events = snapshot.events;
  assert.deepEqual(events.map(e => [e.kind, e.blockNumber]), [
    ['deposit', 9007199254740993n], ['withdrawal', 9007199254740994n],
  ]);
  assert.equal(events[0].commitment, field(1));
  assert.equal(events[0].leafIndex, 0);
  assert.equal(events[1].fee, 18446744073709551617n);
  assert.equal(events[1].to, addresses.recipient);
  const bounded = own(await remote.syncRange(instance, 9007199254740993n, 9007199254740994n));
  assert.equal(bounded.toBlock, 9007199254740994n);
  assert.deepEqual(bounded.events, [events[0]], 'the exclusive upper bound must exclude the withdrawal');
  assert.deepEqual(requested.sort(), [
    '/1_ETH_0.1_deposits.ndjson', '/1_ETH_0.1_deposits.ndjson',
    '/1_ETH_0.1_nullifiers.ndjson', '/1_ETH_0.1_nullifiers.ndjson',
  ]);
  const tree = own(new wasm.MerkleTree());
  tree.spliceEvents(snapshot);
  assert.equal(own(tree.proof(0)).leaf, field(1));
  assert.equal(snapshot.events.length, 2, 'splicing must not consume the snapshot');
});

for (const failure of ['http', 'malformed']) {
  test(`remote cache rejects ${failure} failure instead of reporting an empty snapshot`, async t => {
    const own = ownership(t);
    const instance = pool(own);
    let calls = 0;
    const url = await server(t, (req, res) => {
      calls++;
      assert.ok(['/1_ETH_0.1_deposits.ndjson', '/1_ETH_0.1_nullifiers.ndjson'].includes(req.url));
      if (failure === 'http') res.writeHead(500);
      // Empty text is valid NDJSON: ignoring the HTTP status must not pass via a parser error.
      res.end(failure === 'http' ? '' : 'not ndjson');
    });
    const remote = own(new wasm.RemoteSyncer(url));
    await assert.rejects(remote.syncRange(instance, 0n, 10n), failure === 'http' ? /500/ : /JSON|Json|expected/);
    assert.equal(calls, 1);
  });
}

test('remote cache rejects numeric and out-of-range block arguments before any HTTP request', async t => {
  const own = ownership(t);
  const instance = pool(own);
  let calls = 0;
  const url = await server(t, (req, res) => {
    calls++;
    res.setHeader('content-type', 'application/x-ndjson');
    res.end('');
  });
  const remote = own(new wasm.RemoteSyncer(url));
  for (const [from, to] of [[0, 10n], [0n, 10], [-1n, 10n], [0n, 1n << 64n]]) {
    await assert.rejects(remote.syncRange(instance, from, to), /bigint/);
  }
  assert.equal(calls, 0);
});
