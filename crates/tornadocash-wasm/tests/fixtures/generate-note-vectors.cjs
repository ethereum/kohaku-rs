// Run only when reviewing/updating reference vectors. Never call this from the tests.
// Usage: node generate-note-vectors.cjs /absolute/path/to/node_modules/circomlibjs
const { resolve } = require('node:path');
if (!process.argv[2]) throw new Error('Provide the path to circomlibjs@0.1.7');
const reference = resolve(process.argv[2]);
const { version } = require(`${reference}/package.json`);
if (version !== '0.1.7') throw new Error(`Expected circomlibjs 0.1.7, received ${version}`);
const { buildPedersenHash } = require(reference);

(async () => {
  const pedersen = await buildPedersenHash();
  const hash = (bytes) => {
    const point = pedersen.babyJub.unpackPoint(pedersen.hash(bytes));
    return `0x${pedersen.babyJub.F.toObject(point[0]).toString(16).padStart(64, '0')}`;
  };
  const inputs = [
    ['repeated bytes', Buffer.alloc(31, 1), Buffer.alloc(31, 2)],
    ['ordered bytes', Buffer.from(Array.from({ length: 31 }, (_, i) => i)),
      Buffer.from(Array.from({ length: 31 }, (_, i) => 255 - i))],
    ['zero bytes', Buffer.alloc(31), Buffer.alloc(31)],
  ];
  const vectors = inputs.map(([name, nullifier, secret]) => ({
    name,
    nullifier: nullifier.toString('hex'),
    secret: secret.toString('hex'),
    commitment: hash(Buffer.concat([nullifier, secret])),
    nullifierHash: hash(nullifier),
  }));
  console.log(JSON.stringify({ source: 'circomlibjs@0.1.7, Pedersen BabyJubJub x-coordinate', vectors }, null, 2));
})().catch((error) => { console.error(error); process.exitCode = 1; });
