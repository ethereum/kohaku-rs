import { afterAll, inject, test } from 'vitest';
import { loadSdk, loadArtifactBytes } from '#runtime';

// Fixed, bounded repetitions also bound fresh browser module/WASM instances.
const iterations = inject('smoke') ? 1 : 5;
const options = name => ({
  perProject: true,
  writeResult: `${inject('resultPath')}/${name}.json`,
});
let sdk, bytes, artifacts, asset, pool, note, tree, proof, withdrawal;
async function initialize() {
  sdk ??= await loadSdk();
  bytes ??= await loadArtifactBytes();
}
afterAll(() => {
  withdrawal?.free(); proof?.free(); tree?.free(); note?.free();
  pool?.free(); asset?.free(); artifacts?.free();
});

// Use a fresh generated module; artifacts are deliberately not loaded here.
test('cold start: generated bindings and WASM initialization', async ({ bench }) => {
  let fresh;
  await bench('cold-start', { ...options('cold-start'), afterEach() {
    // Consume the initialized API without adding SDK operations to its timing.
    if (fresh && typeof fresh.Note !== 'function') throw new Error('SDK did not initialize');
    fresh = undefined;
  } }, async () => { fresh = await loadSdk(); }).run({ iterations, time: 0, warmup: false });
});

test('load artifacts: CircuitArtifacts.fromBytes', async ({ bench }) => {
  await initialize();
  let loaded;
  await bench('load-artifacts', { ...options('load-artifacts'), afterEach() {
    loaded?.free(); loaded = undefined;
  } }, () => { loaded = sdk.CircuitArtifacts.fromBytes(bytes); }).run({ iterations, time: 0, warmup: false });
});

test('proving: Withdrawal.prove', async ({ bench }) => {
  await initialize();
  artifacts = sdk.CircuitArtifacts.fromBytes(bytes);
  asset = sdk.Asset.eth();
  pool = new sdk.Pool(1n, '0x0000000000000000000000000000000000000011', asset,
    100000000000000000n, 0n);
  note = new sdk.Note(`0x${'01'.repeat(31)}`, `0x${'02'.repeat(31)}`);
  tree = sdk.MerkleTree.fromLeaves([note.commitment()]);
  proof = tree.proof(0);
  withdrawal = new sdk.Withdrawal(pool, note, '0x0000000000000000000000000000000000000022');
  let proven;
  await bench('proving', { ...options('proving'), afterEach() {
    // Consume the unsigned call and release its proof outside the timed operation.
    if (proven) { proven.data(); proven.free(); proven = undefined; }
  } }, () => { proven = withdrawal.prove(proof, artifacts); }).run({ iterations, time: 0, warmup: false });
});

for (const batchSize of [1, 128]) {
  test(`merkle insertion: ${batchSize} leaves into a 1024-leaf tree`, async ({ bench }) => {
    await initialize();
    const hex = n => `0x${n.toString(16).padStart(64, '0')}`;
    const leaves = Array.from({ length: 1024 }, (_, i) => hex(i + 1));
    const batch = Array.from({ length: batchSize }, (_, i) => hex(1025 + i));
    let current;
    await bench(`merkle-insert-${batchSize}`, {
      ...options(`merkle-insert-${batchSize}`),
      beforeEach() { current = sdk.MerkleTree.fromLeaves(leaves); },
      afterEach() { current.root(); current.free(); current = undefined; },
    }, () => { current.splice(1024, batch); }).run({ iterations, time: 0, warmup: false });
  });
}
