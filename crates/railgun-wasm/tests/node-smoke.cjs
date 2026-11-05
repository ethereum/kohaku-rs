const assert = require('node:assert/strict');
const http = require('node:http');
const { Chain, PoiSession } = require('../../target/railgun-wasm-node/kohaku_railgun_wasm.js');

const server = http.createServer(async (request, response) => {
  let body = '';
  for await (const chunk of request) body += chunk;
  const rpc = JSON.parse(body);
  response.setHeader('content-type', 'application/json');
  response.end(JSON.stringify({ jsonrpc: '2.0', id: rpc.id, result: '0xaa36a7' }));
});

(async () => {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    const rpc = `http://127.0.0.1:${server.address().port}`;
    const chain = Chain.sepolia();
    const key1 = `0x${'01'.repeat(32)}`;
    const key2 = `0x${'02'.repeat(32)}`;
    const first = await PoiSession.open(chain, rpc, key1, key2);
    assert.match(first.address, /^0zk/);
    assert.equal(first.syncedBlock, 0n);
    assert.equal(JSON.parse(await first.balancesJson()).length, 0);
    assert.equal(JSON.parse(await first.notesJson()).length, 0);
    assert.equal(JSON.parse(first.poiPendingJson()).length, 0);
    const report = JSON.parse(await first.processPendingPoiJson());
    assert.equal(report.pendingBefore, 0);
    assert.equal(report.pendingAfter, 0);
    const checkpoint = first.checkpointJson();
    const restored = await PoiSession.open(chain, rpc, key1, key2, checkpoint);
    assert.equal(restored.address, first.address);
    assert.deepEqual(JSON.parse(restored.checkpointJson()), JSON.parse(checkpoint));
    await assert.rejects(PoiSession.open(chain, rpc, '0x12', key2), /32 bytes/);
    const wrongChain = Chain.mainnet();
    await assert.rejects(PoiSession.open(wrongChain, rpc, key1, key2), /chain ID/);
    wrongChain.free();
    restored.free();
    first.free();
    chain.free();
  } finally {
    server.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
