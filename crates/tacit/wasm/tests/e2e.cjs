// Runs the Node bindings end to end against the pool's bytecode on a fresh anvil: keys, proving,
// shield, private transfer, relayer-paid withdrawal, save and restore.
//
//   cargo build -p kohaku-tacit-wasm --target wasm32-unknown-unknown --release
//   wasm-bindgen target/wasm32-unknown-unknown/release/kohaku_tacit_wasm.wasm \
//     --target nodejs --out-dir target/tacit-wasm-node
//   TACIT_ARTIFACTS_DIR=<dir with transact.wasm, transact_final.zkey> \
//     node tacit/wasm/tests/e2e.cjs target/tacit-wasm-node
//
// Needs Node 22 or later and anvil on the PATH.
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const assert = require('assert/strict');

const W = require(path.resolve(process.argv[2], 'kohaku_tacit_wasm.js'));
const ART = process.env.TACIT_ARTIFACTS_DIR;
const RS = path.join(__dirname, '..', '..');
const fixture = JSON.parse(fs.readFileSync(`${RS}/core/tests/fixtures/tacit_pool.json`, 'utf8'));
const vectors = JSON.parse(fs.readFileSync(`${RS}/circuit/tests/fixtures/reference-vectors.json`, 'utf8'));
const ETH = 10n ** 18n;
const port = 20000 + Math.floor(Math.random() * 2000);
const url = `http://127.0.0.1:${port}`;

async function rpc(method, params = []) {
  const r = await fetch(url, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }) });
  const j = await r.json();
  if (j.error) throw new Error(`${method}: ${JSON.stringify(j.error)}`);
  return j.result;
}
async function send(tx) {
  const hash = await rpc('eth_sendTransaction', [tx]);
  for (;;) {
    const receipt = await rpc('eth_getTransactionReceipt', [hash]);
    if (receipt) {
      if (receipt.status !== '0x1') throw new Error('reverted');
      return receipt;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
}
const word = (hex) => hex.replace(/^0x/, '').padStart(64, '0');
const hexOf = (n) => `0x${n.toString(16)}`;
const timed = (label, f) => { const t = Date.now(); const out = f(); console.log(`${label}: ${((Date.now() - t) / 1000).toFixed(1)} s`); return out; };

(async () => {
  const anvil = spawn('anvil', ['--port', String(port), '--silent'], { stdio: 'ignore' });
  try {
    for (let i = 0; ; i++) {
      try { await rpc('eth_chainId'); break; } catch (e) { if (i > 100) throw e; await new Promise((r) => setTimeout(r, 100)); }
    }
    const [from] = await rpc('eth_accounts');
    const verifier = (await send({ from, data: fixture.verifier.bytecode })).contractAddress;
    const deployed = await send({ from, data: fixture.pool.bytecode + word(verifier) + word('0x0') });
    const chainId = BigInt(await rpc('eth_chainId'));
    const pool = new W.TacitPool(chainId, deployed.contractAddress, BigInt(deployed.blockNumber)).withConfirmations(0);

    // Keys agree with the reference client and with Kohaku's keystore.
    const alice = W.TacitAccount.fromIdentity(Buffer.from('11'.repeat(32), 'hex'));
    assert.equal(alice.address, vectors.identity1.address);
    const phrase = 'test test test test test test test test test test test junk';
    assert.equal(W.TacitAccount.fromMnemonic(phrase, 0).address,
      W.TacitAccount.fromIdentity(Buffer.from('dd3bc2f7f6a3589b25b7bf1d2c553b1419d6435a731eb192047b55a6f61a3363', 'hex')).address);
    console.log('ok keys match the reference and the keystore');

    assert.throws(() => new W.TacitProver(Buffer.from('nope'), Buffer.from('nope')), /pinned hash/);
    const prover = timed('prover loaded', () => new W.TacitProver(fs.readFileSync(`${ART}/transact.wasm`), fs.readFileSync(`${ART}/transact_final.zkey`)));

    const a = new W.TacitWallet(alice, pool);
    const b = new W.TacitWallet(W.TacitAccount.fromIdentity(Buffer.from('22'.repeat(32), 'hex')), pool);
    await a.sync(url);

    const shield = a.shield(ETH / 10n);
    assert.equal(shield.kind, 'shield');
    const call = timed('shield proved', () => shield.prove(prover)).call();
    assert.equal(call.value, ETH / 10n);
    await send({ from, to: call.to, data: call.data, value: hexOf(call.value) });
    await a.sync(url);
    assert.equal(a.balance(), ETH / 10n);
    assert.equal(a.notes().length, 1);
    console.log('ok shield lands and the wallet finds its note');

    const pay = a.transfer(b.address, 3n * ETH / 100n, W.TacitPayer.own(), a.tree());
    await send({ from, ...(({ to, data }) => ({ to, data }))(timed('transfer proved', () => pay.prove(prover)).call()) });
    await Promise.all([a.sync(url), b.sync(url)]);
    assert.equal(a.balance(), 7n * ETH / 100n);
    assert.equal(b.balance(), 3n * ETH / 100n);
    console.log('ok private transfer: both wallets see it');

    const relayer = '0x00000000000000000000000000000000000000ee';
    const recipient = '0x00000000000000000000000000000000000000cc';
    const payer = new W.TacitPayer(relayer, ETH / 10000n);
    const out = b.unshield(recipient, ETH / 100n, payer, b.tree());
    const proven = timed('withdrawal proved', () => out.prove(prover));
    const request = proven.relayRequest();
    assert.equal(request.fee, String(ETH / 10000n));
    assert.equal(request.publicInputs.length, 11);
    await send({ from, to: proven.call().to, data: proven.call().data });
    assert.equal(BigInt(await rpc('eth_getBalance', [recipient, 'latest'])), ETH / 100n);
    assert.equal(BigInt(await rpc('eth_getBalance', [relayer, 'latest'])), ETH / 10000n);
    await b.sync(url);
    assert.equal(b.balance(), 3n * ETH / 100n - ETH / 100n - ETH / 10000n);
    console.log('ok relayer-paid withdrawal: recipient and relayer paid, change kept');

    const restored = W.TacitWallet.restore(alice, pool, a.save());
    assert.equal(restored.balance(), a.balance());
    await Promise.all([restored.sync(url), a.sync(url)]);
    assert.equal(restored.tree().root, a.tree().root);
    console.log('ok save and restore');
    console.log('ALL OK');
  } finally {
    anvil.kill();
  }
})().catch((e) => { console.error('FAIL', e); process.exit(1); });
