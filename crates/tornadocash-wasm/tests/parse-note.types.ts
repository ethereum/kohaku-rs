import { Note, type NoteData } from '../../target/tornadocash-wasm-node/kohaku_tornadocash_wasm';

// Compile-only consumer of the generated declarations; this file is not executed.
const instance: Note = Note.parse('synthetic note supplied at runtime');
const note = instance.toObject();
const parsed: NoteData = note;
const symbol: string = parsed.symbol;
const amount: string = parsed.amount;
const chainId: bigint = note.chainId;
const nullifier: Uint8Array = note.nullifier;
const secret: Uint8Array = note.secret;

// @ts-expect-error Chain IDs must not silently become numbers or any.
const numericChainId: number = note.chainId;
// @ts-expect-error Secret bytes are typed arrays, not ordinary number arrays.
const secretArray: number[] = note.secret;
// @ts-expect-error Rust snake_case is converted to camelCase in the public object.
note.chain_id;

const commitment: string = instance.commitment();
const nullifierHash: string = instance.nullifierHash();
const text: string = instance.toString();
const preimage: Uint8Array = instance.preimage();

// @ts-expect-error Preimage returns a typed array, not a hex string or any.
const preimageHex: string = instance.preimage();
// @ts-expect-error Preimage bytes are not an ordinary number array.
const preimageArray: number[] = instance.preimage();

// @ts-expect-error Parsing accepts text, not structured data.
Note.parse(parsed);
// @ts-expect-error Parsing returns a class instance, not a plain DTO or any.
const data: NoteData = Note.parse('note');
// @ts-expect-error The snapshot has no WASM methods.
note.commitment();
// @ts-expect-error Hash methods return strings, not bigint or any.
const hashNumber: bigint = instance.commitment();

instance.free();
