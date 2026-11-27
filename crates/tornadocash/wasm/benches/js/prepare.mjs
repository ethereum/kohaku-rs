import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { brotliDecompressSync } from 'node:zlib';

const target = resolve(import.meta.dirname, '../../../../target');
const assets = resolve(import.meta.dirname, 'generated');
mkdirSync(resolve(assets, 'bindings'), { recursive: true });
for (const name of ['kohaku_tornadocash_wasm.js', 'kohaku_tornadocash_wasm_bg.wasm']) {
  copyFileSync(resolve(target, 'tornadocash-wasm-web', name), resolve(assets, 'bindings', name));
}
writeFileSync(resolve(assets, 'artifacts.bin'), brotliDecompressSync(readFileSync(
  resolve(import.meta.dirname, '../../../circuit/data/artifacts.bin.br'),
)));
console.log(`Benchmark assets prepared; Node ${process.version}, ${process.platform}/${process.arch}`);
