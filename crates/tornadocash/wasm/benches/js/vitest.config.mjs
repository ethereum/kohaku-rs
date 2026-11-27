import { resolve } from 'node:path';
import { defineConfig } from 'vitest/config';
import { playwright } from '@vitest/browser-playwright';

const root = import.meta.dirname;
const target = resolve(root, '../../../../target');
const assets = resolve(root, 'generated');
const smoke = process.env.KOHAKU_BENCH_SMOKE === '1';

export default defineConfig({
  root,
  publicDir: false,
  test: {
    include: [],
    fileParallelism: false,
    maxWorkers: 1,
    testTimeout: 120000,
    provide: {
      smoke,
      nodeBindingsFile: resolve(target, 'tornadocash-wasm-node/kohaku_tornadocash_wasm.js'),
      artifactFile: resolve(assets, 'artifacts.bin'),
      resultPath: 'results/${projectName}',
    },
    benchmark: { include: ['sdk.bench.mjs'], retainSamples: true },
    projects: [
      {
        resolve: { alias: { '#runtime': resolve(root, 'runtime.node.mjs') } },
        test: { name: 'node', environment: 'node' },
      },
      {
        resolve: { alias: { '#runtime': resolve(root, 'runtime.browser.mjs') } },
        test: {
          name: 'chromium',
          browser: {
            enabled: true,
            headless: true,
            provider: playwright({ launchOptions: {
              executablePath: process.env.KOHAKU_CHROMIUM_EXECUTABLE,
            } }),
            instances: [{ browser: 'chromium' }],
          },
        },
      },
    ],
  },
});
