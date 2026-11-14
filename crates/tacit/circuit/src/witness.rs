//! The circuit's witness program: the WebAssembly circom 2 compiles alongside the constraints, run
//! in `wasmi`, a pure-Rust interpreter that also builds for `wasm32`.
//!
//! The program's interface is circom's: inputs are named by the FNV-1a hash of the signal name and
//! set one field element at a time through a shared buffer of 32-bit limbs, least significant
//! first.

use ark_bn254::Fr;
use ark_ff::{BigInt, PrimeField};
use wasmi::{Caller, Engine, Linker, Module, Store, TypedFunc, WasmParams, WasmResults};

use crate::CircuitError;

/// The compiled witness program.
pub(crate) struct WitnessProgram {
    engine: Engine,
    module: Module,
}

/// Messages the program printed before it failed.
#[derive(Default)]
struct Host {
    message: String,
}

const LIMBS: i32 = 8;

fn fnv1a(name: &str) -> (i32, i32) {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        h ^= u64::from(byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    #[allow(clippy::cast_possible_wrap)]
    ((h >> 32) as u32 as i32, h as u32 as i32)
}

fn exception(code: i32) -> &'static str {
    match code {
        1 => "an input signal is not part of the circuit",
        2 => "an input signal was given too many values",
        3 => "an input signal was set twice",
        4 => "a constraint of the circuit does not hold for these inputs",
        5 => "the witness program ran out of memory",
        6 => "an input value is not a field element",
        _ => "the witness program failed",
    }
}

/// The program's exports this runner uses.
struct Exports {
    field_limbs: TypedFunc<(), i32>,
    raw_prime: TypedFunc<(), ()>,
    read: TypedFunc<i32, i32>,
    write: TypedFunc<(i32, i32), ()>,
    init: TypedFunc<i32, ()>,
    signal_size: TypedFunc<(i32, i32), i32>,
    set_signal: TypedFunc<(i32, i32, i32), ()>,
    input_size: TypedFunc<(), i32>,
    witness_size: TypedFunc<(), i32>,
    get_witness: TypedFunc<i32, ()>,
}

/// One instance of the program, for one witness.
struct Session {
    store: Store<Host>,
    exports: Exports,
}

impl Session {
    fn call<P: WasmParams, R: WasmResults>(
        &mut self,
        func: TypedFunc<P, R>,
        params: P,
    ) -> Result<R, CircuitError> {
        func.call(&mut self.store, params)
            .map_err(|error| trap(&self.store, &error))
    }

    /// The element in the shared buffer.
    fn read_element(&mut self) -> Result<BigInt<4>, CircuitError> {
        let mut limbs = [0u64; 4];
        for (i, j) in (0..LIMBS).enumerate() {
            #[allow(clippy::cast_sign_loss)]
            let limb = u64::from(self.call(self.exports.read, j)? as u32);
            limbs[i / 2] |= limb << (32 * (i % 2));
        }
        Ok(BigInt(limbs))
    }

    fn write_element(&mut self, value: Fr) -> Result<(), CircuitError> {
        let limbs = value.into_bigint().0;
        for (i, j) in (0..LIMBS).enumerate() {
            #[allow(clippy::cast_possible_wrap)]
            let limb = (limbs[i / 2] >> (32 * (i % 2))) as u32 as i32;
            self.call(self.exports.write, (j, limb))?;
        }
        Ok(())
    }

    fn check_field(&mut self) -> Result<(), CircuitError> {
        if self.call(self.exports.field_limbs, ())? != LIMBS {
            return Err(CircuitError::WitnessProgram("unexpected field size".into()));
        }
        self.call(self.exports.raw_prime, ())?;
        if self.read_element()? != Fr::MODULUS {
            return Err(CircuitError::WitnessProgram("not a BN254 circuit".into()));
        }
        Ok(())
    }

    fn set_inputs(&mut self, inputs: &[(&str, Vec<Fr>)]) -> Result<(), CircuitError> {
        self.call(self.exports.init, 0)?;
        let mut set = 0usize;
        for (name, values) in inputs {
            let (hi, lo) = fnv1a(name);
            let size = self.call(self.exports.signal_size, (hi, lo))?;
            if usize::try_from(size).ok() != Some(values.len()) {
                return Err(CircuitError::InputSize {
                    signal: (*name).to_string(),
                    expected: usize::try_from(size).unwrap_or(0),
                    actual: values.len(),
                });
            }
            for (i, value) in (0..).zip(values) {
                self.write_element(*value)?;
                self.call(self.exports.set_signal, (hi, lo, i))?;
                set += 1;
            }
        }
        // The program does not refuse an input left out; it would prove with that signal at zero.
        let expected = self.call(self.exports.input_size, ())?;
        if usize::try_from(expected).ok() != Some(set) {
            return Err(CircuitError::WitnessProgram(format!(
                "{set} input values given, the circuit takes {expected}"
            )));
        }
        Ok(())
    }

    fn witness(&mut self) -> Result<Vec<Fr>, CircuitError> {
        let size = self.call(self.exports.witness_size, ())?;
        (0..size)
            .map(|i| {
                self.call(self.exports.get_witness, i)?;
                Fr::from_bigint(self.read_element()?)
                    .ok_or_else(|| CircuitError::WitnessProgram("witness out of range".into()))
            })
            .collect()
    }
}

impl WitnessProgram {
    pub(crate) fn new(wasm: &[u8]) -> Result<Self, CircuitError> {
        let engine = Engine::default();
        let module = Module::new(&engine, wasm).map_err(|e| program(&e))?;
        Ok(Self { engine, module })
    }

    fn session(&self) -> Result<Session, CircuitError> {
        let mut store = Store::new(&self.engine, Host::default());
        let mut linker = Linker::<Host>::new(&self.engine);
        linker
            .func_wrap("runtime", "exceptionHandler", |code: i32| {
                Err::<(), _>(wasmi::Error::i32_exit(code))
            })
            .and_then(|l| {
                l.func_wrap(
                    "runtime",
                    "printErrorMessage",
                    |mut caller: Caller<'_, Host>| {
                        let message = read_message(&mut caller);
                        caller.data_mut().message.push_str(&message);
                    },
                )
            })
            .and_then(|l| l.func_wrap("runtime", "writeBufferMessage", |_: Caller<'_, Host>| {}))
            .and_then(|l| l.func_wrap("runtime", "showSharedRWMemory", |_: Caller<'_, Host>| {}))
            .map_err(|e| CircuitError::WitnessProgram(e.to_string()))?;
        let instance = linker
            .instantiate_and_start(&mut store, &self.module)
            .map_err(|e| program(&e))?;
        let exports = Exports {
            field_limbs: func(instance, &store, "getFieldNumLen32")?,
            raw_prime: func(instance, &store, "getRawPrime")?,
            read: func(instance, &store, "readSharedRWMemory")?,
            write: func(instance, &store, "writeSharedRWMemory")?,
            init: func(instance, &store, "init")?,
            signal_size: func(instance, &store, "getInputSignalSize")?,
            set_signal: func(instance, &store, "setInputSignal")?,
            input_size: func(instance, &store, "getInputSize")?,
            witness_size: func(instance, &store, "getWitnessSize")?,
            get_witness: func(instance, &store, "getWitness")?,
        };
        Ok(Session { store, exports })
    }

    /// The full witness for `inputs`, each a signal name and its values in row-major order.
    pub(crate) fn calculate(&self, inputs: &[(&str, Vec<Fr>)]) -> Result<Vec<Fr>, CircuitError> {
        let mut session = self.session()?;
        session.check_field()?;
        session.set_inputs(inputs)?;
        session.witness()
    }
}

fn func<P: WasmParams, R: WasmResults>(
    instance: wasmi::Instance,
    store: &Store<Host>,
    name: &str,
) -> Result<TypedFunc<P, R>, CircuitError> {
    instance
        .get_typed_func::<P, R>(store, name)
        .map_err(|_| CircuitError::WitnessProgram(format!("missing export {name}")))
}

/// The message the program writes before calling `printErrorMessage`, one character at a time.
fn read_message(caller: &mut Caller<'_, Host>) -> String {
    let Some(next) = caller
        .get_export("getMessageChar")
        .and_then(wasmi::Extern::into_func)
        .and_then(|f| f.typed::<(), i32>(&*caller).ok())
    else {
        return String::new();
    };
    let mut out = String::new();
    while let Ok(c) = next.call(&mut *caller, ()) {
        if c == 0 || out.len() > 1024 {
            break;
        }
        #[allow(clippy::cast_sign_loss)]
        out.push(char::from_u32(c as u32).unwrap_or('?'));
    }
    out.push('\n');
    out
}

fn trap(store: &Store<Host>, error: &wasmi::Error) -> CircuitError {
    let printed = store.data().message.trim();
    let reason = match error.i32_exit_status() {
        Some(code) => exception(code).to_string(),
        None => error.to_string(),
    };
    if printed.is_empty() {
        CircuitError::Witness(reason)
    } else {
        CircuitError::Witness(format!("{reason}: {printed}"))
    }
}

fn program(error: &wasmi::Error) -> CircuitError {
    CircuitError::WitnessProgram(error.to_string())
}
