import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { after, afterEach, before, beforeEach, test } from 'node:test';
import { chromium } from 'playwright';

const generated = new URL('../../../target/tornadocash-wasm-web/', import.meta.url);
const { vectors } = JSON.parse(await readFile(new URL('../fixtures/note-vectors.json', import.meta.url)));
let server;
let browser;
let page;
let origin;

before(async () => {
  const routes = new Map([['/', ['text/html', '<!doctype html><title>Note binding tests</title>']]]);
  for (const [name, type] of [
    ['kohaku_tornadocash_wasm.js', 'text/javascript'],
    ['kohaku_tornadocash_wasm_bg.wasm', 'application/wasm'],
  ]) {
    routes.set(`/${name}`, [type, await readFile(new URL(name, generated))]);
  }
  server = createServer((request, response) => {
    const route = routes.get(request.url);
    if (!route) return response.writeHead(404).end();
    response.writeHead(200, { 'Content-Type': route[0] }).end(route[1]);
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  origin = `http://127.0.0.1:${server.address().port}`;
  browser = await chromium.launch({ executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH });
});

after(async () => {
  await browser?.close();
  if (server?.listening) await new Promise((resolve, reject) => {
    server.close(error => error ? reject(error) : resolve());
  });
});

beforeEach(async () => {
  page = await browser.newPage();
  await page.goto(origin);
  await page.evaluate(async () => {
    const bindings = await import('/kohaku_tornadocash_wasm.js');
    await bindings.default();
    globalThis.Note = bindings.Note;
  });
});
afterEach(async () => { await page?.close(); });

test('web bindings preserve structured types, bytes and reference hashes', { timeout: 60_000 }, async () => {
  const actual = await page.evaluate(vectors => vectors.map(vector => {
    const fromHex = hex => Uint8Array.from(hex.match(/../g), byte => parseInt(byte, 16));
    const note = new Note({
      symbol: 'eth', amount: '0.10', chainId: 18446744073709551615n,
      nullifier: fromHex(vector.nullifier), secret: fromHex(vector.secret),
    });
    try {
      const data = note.toObject();
      const text = note.toString();
      const parsed = Note.parse(text);
      try {
        return {
          plainObject: Object.getPrototypeOf(data) === Object.prototype,
          chainIdType: typeof data.chainId,
          chainId: data.chainId.toString(),
          typedBytes: data.nullifier instanceof Uint8Array && data.secret instanceof Uint8Array,
          preimage: Array.from(note.preimage()),
          text,
          commitment: note.commitment(), nullifierHash: note.nullifierHash(),
          parsedCommitment: parsed.commitment(), parsedNullifierHash: parsed.nullifierHash(),
        };
      } finally { parsed.free(); }
    } finally { note.free(); }
  }), vectors);
  for (const [i, result] of actual.entries()) {
    const vector = vectors[i];
    assert.deepEqual(result, {
      plainObject: true, chainIdType: 'bigint', chainId: '18446744073709551615', typedBytes: true,
      preimage: Array.from(Buffer.from(vector.nullifier + vector.secret, 'hex')),
      text: `tornado-eth-0.10-18446744073709551615-0x${vector.nullifier}${vector.secret}`,
      commitment: vector.commitment, nullifierHash: vector.nullifierHash,
      parsedCommitment: vector.commitment, parsedNullifierHash: vector.nullifierHash,
    }, vector.name);
  }
});

test('web bindings reject invalid inputs with JavaScript Errors', async () => {
  const results = await page.evaluate(() => {
    const fields = { symbol: 'eth', amount: '1', chainId: 1n, nullifier: new Uint8Array(31), secret: new Uint8Array(31) };
    const calls = [
      ['parse: invalid text', () => Note.parse('invalid')],
      ...[
        ['number', 1],
        ['negative bigint', -1n],
        ['bigint above u64', 18446744073709551616n],
      ].flatMap(([name, chainId]) => [
        [`constructor: ${name}`, () => new Note({ ...fields, chainId })],
        [`random: ${name}`, () => Note.random('eth', '1', chainId)],
      ]),
      ...['nullifier', 'secret'].flatMap(field =>
        [
          ['plain array', Array(31).fill(0)],
          ['30 bytes', new Uint8Array(30)],
          ['32 bytes', new Uint8Array(32)],
        ].map(([name, value]) => [`${field}: ${name}`, () => new Note({ ...fields, [field]: value })])),
    ];
    return calls.map(([name, call]) => {
      let note;
      try { note = call(); }
      catch (error) { return { name, isError: error instanceof Error, message: String(error) }; }
      note.free();
      return { name, isError: false, message: 'input was accepted' };
    });
  });
  for (const { name, isError, message } of results) {
    assert.ok(isError, `${name}: ${message}`);
  }
});

test('web bindings use Web Crypto, report failures, and recover', async () => {
  const result = await page.evaluate(() => {
    const original = Object.getOwnPropertyDescriptor(globalThis, 'crypto');
    const failures = [];
    try {
      const cases = [
        ['missing Web Crypto', undefined],
        ['failing Web Crypto', { getRandomValues() { throw new Error('unavailable'); } }],
      ];
      for (const [name, crypto] of cases) {
        Object.defineProperty(globalThis, 'crypto', { configurable: true, value: crypto });
        let note;
        try { note = Note.random('eth', '1', 1n); }
        catch (error) {
          failures.push({ name, isError: error instanceof Error, message: String(error) });
          continue;
        }
        note.free();
        failures.push({ name, isError: false, message: 'generation succeeded' });
      }
    } finally {
      if (original) Object.defineProperty(globalThis, 'crypto', original);
      else delete globalThis.crypto;
    }
    const first = Note.random('eth', '0.10', 1n);
    try {
      const second = Note.random('eth', '0.10', 1n);
      try {
        const parsed = Note.parse(first.toString());
        try {
          return {
            secureContext: isSecureContext,
            failures,
            distinct: first.toString() !== second.toString(),
            roundTrip: parsed.toString() === first.toString(),
            preimageLength: first.preimage().length,
          };
        } finally { parsed.free(); }
      } finally { second.free(); }
    } finally { first.free(); }
  });
  const { failures, ...generation } = result;
  for (const { name, isError, message } of failures) {
    assert.ok(isError, `${name}: ${message}`);
  }
  assert.deepEqual(generation, { secureContext: true, distinct: true, roundTrip: true, preimageLength: 62 });
});
