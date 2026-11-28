import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { inject } from 'vitest';

const require = createRequire(import.meta.url);

export function loadSdk() {
  const file = inject('nodeBindingsFile');
  delete require.cache[require.resolve(file)];
  return require(file);
}

export function loadArtifactBytes() {
  return new Uint8Array(readFileSync(inject('artifactFile')));
}
