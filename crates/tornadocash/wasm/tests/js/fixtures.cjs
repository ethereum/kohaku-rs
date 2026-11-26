const { readFileSync } = require('node:fs');
const path = require('node:path');
const { brotliDecompressSync } = require('node:zlib');

// Existing circuit/benches/signals.json vector: 31-byte secrets are little-endian.
const vector = Object.freeze({
  nullifier: '0x58fd9969e8f5f3ab7aabe855710c2d2fa2e05ea8adeb542e753a89b834ab3c',
  secret: '0xf202d2f9faf469831aee95c9260f9b43812eb42bb2512df4e850734c8299ca',
  nullifierHash: '0x2282919a73857effb2b2ff117a1e89557f38ea717ea153b48ae893d1c7208fe2',
});
const addresses = Object.freeze({
  pool: '0x0000000000000000000000000000000000000011',
  token: '0x0000000000000000000000000000000000000022',
  recipient: '0x0000000000000000000000000000000000000033',
  payer: '0x0000000000000000000000000000000000000044',
});
const word = value => BigInt(value).toString(16).padStart(64, '0');
const field = value => `0x${word(value)}`;

function artifactBytes() {
  return brotliDecompressSync(readFileSync(path.resolve(
    __dirname, '../../../circuit/data/artifacts.bin.br',
  )));
}

module.exports = { vector, addresses, word, field, artifactBytes };
