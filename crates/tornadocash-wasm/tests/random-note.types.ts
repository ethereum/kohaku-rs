import { Note, type NoteData } from '../../target/tornadocash-wasm-node/kohaku_tornadocash_wasm';

// Compile-only consumer of the generated declarations.
const note: Note = Note.random('eth', '0.10', 1n);
const data: NoteData = note.toObject();
const text: string = note.toString();

// @ts-expect-error Chain IDs must be bigint.
Note.random('eth', '0.10', 1);
// @ts-expect-error Amounts remain strings.
Note.random('eth', 0.1, 1n);
// @ts-expect-error All three metadata arguments are required.
Note.random('eth', '0.10');
// @ts-expect-error Random returns a Note instance, not plain data or any.
const fields: NoteData = Note.random('eth', '0.10', 1n);

note.free();
