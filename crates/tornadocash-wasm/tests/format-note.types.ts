import { Note, type NoteData } from '../../target/tornadocash-wasm-node/kohaku_tornadocash_wasm';

const fields: NoteData = {
  symbol: 'eth',
  amount: '0.10',
  chainId: 1n,
  nullifier: new Uint8Array(31),
  secret: new Uint8Array(31),
};

const note: Note = new Note(fields);
const text: string = note.toString();
const parsed: Note = Note.parse(text);
const roundTrip: string = parsed.toString();
const snapshot: NoteData = note.toObject();

// @ts-expect-error The constructor accepts structured data, not a note string.
new Note(text);
// @ts-expect-error Chain IDs are bigint in the public TypeScript contract.
new Note({ ...fields, chainId: 1 });
// @ts-expect-error Secret bytes are Uint8Array in the public TypeScript contract.
new Note({ ...fields, secret: [1, 2, 3] });
// @ts-expect-error All note fields are required.
new Note({ symbol: 'eth' });
// @ts-expect-error Formatting returns a string, not NoteData or any.
const object: NoteData = note.toString();

note.free();
parsed.free();
