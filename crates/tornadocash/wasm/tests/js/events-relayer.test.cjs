const assert = require('node:assert/strict');
const { test } = require('node:test');
const { wasm, addresses, field, word, ownership, pool } = require('./helpers.cjs');

// Independently derived with cast keccak on the Solidity event signatures.
const depositTopic = '0xa945e51eec50ab98c161376f0db4cf2aeba3ec92755fe2fcd388bdbbb80ff196';
const withdrawalTopic = '0xe9e508bad6d4c3227e881ca19068f099da81b5164dd6d62b2eaf1e8bc6c34931';

test('event decoding preserves indexed values, bigint fees and blocks beyond safe numbers', () => {
  const deposit = wasm.decodeEvent({
    topics: [depositTopic, field(1)], data: `0x${word(7)}${word(123)}`, blockNumber: 9007199254740993n,
  });
  assert.deepEqual(deposit, { kind: 'deposit', commitment: field(1), leafIndex: 7, blockNumber: 9007199254740993n });
  const withdrawal = wasm.decodeEvent({
    topics: [withdrawalTopic, field(0x44)],
    data: `0x${word(0x33)}${word(2)}${word(1n << 200n)}`, blockNumber: 9007199254740995n,
  });
  assert.deepEqual(withdrawal, {
    kind: 'withdrawal', to: addresses.recipient, nullifierHash: field(2), relayer: addresses.payer,
    fee: 1n << 200n, blockNumber: 9007199254740995n,
  });
});

test('unknown events are absent while malformed known events throw', () => {
  assert.equal(wasm.decodeEvent({ topics: [], data: '0x' }), undefined);
  assert.throws(() => wasm.decodeEvent({ topics: [depositTopic, field(1)], data: '0x', blockNumber: 1n }));
  assert.throws(() => wasm.decodeEvent({ topics: [depositTopic, field(1)], data: `0x${word(0)}${word(1)}` }));
  for (const blockNumber of [1, -1n, 1n << 64n, null]) {
    assert.throws(() => wasm.decodeEvent({ topics: [], data: '0x', blockNumber }));
  }
});

test('empty snapshot bounds remain exact and its borrowed pool remains usable', t => {
  const own = ownership(t);
  const instance = pool(own);
  const snapshot = own(new wasm.Snapshot(instance, 9007199254740993n, 9007199254740995n, []));
  assert.equal(snapshot.fromBlock, 9007199254740993n);
  assert.equal(snapshot.toBlock, 9007199254740995n);
  assert.deepEqual(snapshot.events, []);
  assert.equal(own(snapshot.pool).address, addresses.pool);
  const tree = own(new wasm.MerkleTree());
  const before = tree.root();
  tree.spliceEvents(snapshot);
  assert.equal(tree.root(), before);
  assert.equal(instance.address, addresses.pool);
  assert.throws(() => new wasm.Snapshot(instance, 1, 2n, []));
});

function statusText({ netId = '1', price = '0x1bc16d674ec80000' } = {}) {
  return `{
    "rewardAccount":"${addresses.payer}",
    "instances":{
      "eth":{"instanceAddress":{"0.1":"${addresses.pool}"},"symbol":"eth","decimals":18},
      "TKN":{"instanceAddress":{"100":"${addresses.pool}"},"symbol":"TKN","decimals":18}
    },
    "netId":${netId},"ethPrices":{"TKN":"${price}"},
    "tornadoServiceFee":0.5,"miningServiceFee":0,"version":"test-fixture",
    "health":{"status":"true"},"currentQueue":2
  }`;
}

test('relayer status produces exact native and ERC20 quotes without consuming pools', t => {
  const own = ownership(t);
  const status = own(wasm.RelayerStatus.parse(statusText()));
  const native = pool(own);
  const token = pool(own, { erc20: true, amount: 100000000000000000000n });
  assert.equal(status.supports(native), true);
  assert.equal(status.supports(token), true);
  assert.equal(status.ethPrices.get('TKN'), 2000000000000000000n);
  const nativeQuote = own(status.quote(native, 1000000000n, 0n));
  assert.equal(nativeQuote.address, addresses.payer);
  assert.equal(nativeQuote.fee, 1000000000000000n); // 500,000 gas * 1 gwei + 0.5% * 0.1 ETH.
  assert.equal(nativeQuote.refund, 0n);
  const tokenQuote = own(status.quote(token, 1000000000n, 42n));
  assert.equal(tokenQuote.fee, 500250000000000021n);
  assert.equal(tokenQuote.refund, 42n);
  assert.equal(token.amountWei, 100000000000000000000n);
  const wrongChain = pool(own, { chainId: 2n });
  assert.equal(status.supports(wrongChain), false);
  assert.throws(() => status.quote(wrongChain, 1n, 0n));
});

test('relayer parsing preserves raw large integers and rejects invalid quote inputs', t => {
  const own = ownership(t);
  const status = own(wasm.RelayerStatus.parse(statusText({ netId: '9007199254740993', price: '0x10000000000000001' })));
  assert.equal(status.netId, 9007199254740993n);
  assert.equal(status.ethPrices.get('TKN'), 18446744073709551617n);
  assert.throws(() => wasm.RelayerStatus.parse('{}'));
  const normal = own(wasm.RelayerStatus.parse(statusText()));
  const native = pool(own);
  for (const gas of [1, -1n, 1n << 128n]) assert.throws(() => normal.quote(native, gas, 0n));
  assert.throws(() => normal.quote(native, 1n, 1n << 256n));
  const zeroPrice = own(wasm.RelayerStatus.parse(statusText({ price: '0x0' })));
  const token = pool(own, { erc20: true, amount: 100000000000000000000n });
  assert.throws(() => zeroPrice.quote(token, 1n, 0n));
});
