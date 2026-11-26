const assert = require('node:assert/strict');
const { createServer } = require('node:http');
const path = require('node:path');
const { vector, addresses, word, field, artifactBytes } = require('./fixtures.cjs');

const bindingsPath = process.env.KOHAKU_TORNADO_NODE_BINDINGS || path.resolve(
  __dirname, '../../../../target/tornadocash-wasm-release-node/kohaku_tornadocash_wasm.js',
);
const wasm = require(bindingsPath);


function ownership(t) {
  return value => {
    t.after(() => value.free());
    return value;
  };
}

function pool(own, { erc20 = false, amount = 100000000000000000n, chainId = 1n } = {}) {
  const asset = own(erc20 ? wasm.Asset.erc20(addresses.token, 'TKN', 18) : wasm.Asset.eth());
  return own(new wasm.Pool(chainId, addresses.pool, asset, amount, 0n, undefined));
}

async function server(t, handler) {
  const errors = [];
  const http = createServer(async (req, res) => {
    try {
      await handler(req, res);
    } catch (error) {
      errors.push(error);
      if (!res.headersSent) res.writeHead(500);
      res.end('fixture request did not match');
    }
  });
  await new Promise((resolve, reject) => {
    http.once('error', reject);
    http.listen(0, '127.0.0.1', resolve);
  });
  t.after(async () => {
    await new Promise(resolve => {
      http.close(resolve);
      http.closeAllConnections();
    });
    assert.deepEqual(errors, [], 'fixture received an unexpected request');
  });
  return `http://127.0.0.1:${http.address().port}`;
}

async function jsonBody(req) {
  let body = '';
  for await (const chunk of req) body += chunk;
  return JSON.parse(body);
}

module.exports = { wasm, bindingsPath, vector, addresses, word, field, ownership, pool, artifactBytes, server, jsonBody };
