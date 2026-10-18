//! The scenario operations `tedsid` and `ay`, which mirror the operations
//! of the same names in `reference/tests/parity_generators.inc`.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use solouder_core::Generator;
use solouder_sources::{Ay, ChipDumpError, TedSid};

use crate::files::{FILE_LOAD_FAILED, fixture_path, open_fixture, read_fixture};
use crate::generator_ops::check_generator_keys;
use crate::run::{SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

const CHIP_KEYS: [&str; 3] = ["name", "file", "from"];

/// SoLoud's `INVALID_PARAMETER`.
const INVALID_PARAMETER: i32 = 1;
/// SoLoud's `FILE_NOT_FOUND`.
const FILE_NOT_FOUND: i32 = 2;

/// How `TedSid` and `Ay` load a dump.
trait ChipSource: Generator + Default + Sized {
    fn open(path: &Path) -> Result<Self, ChipDumpError>;
    fn from_bytes(bytes: &[u8]) -> Result<Self, ChipDumpError>;
    fn from_reader(file: File) -> Result<Self, ChipDumpError>;
}

impl ChipSource for TedSid {
    fn open(path: &Path) -> Result<Self, ChipDumpError> {
        TedSid::open(path)
    }
    fn from_bytes(bytes: &[u8]) -> Result<Self, ChipDumpError> {
        TedSid::from_bytes(bytes)
    }
    fn from_reader(file: File) -> Result<Self, ChipDumpError> {
        TedSid::from_reader(file)
    }
}

impl ChipSource for Ay {
    fn open(path: &Path) -> Result<Self, ChipDumpError> {
        Ay::open(path)
    }
    fn from_bytes(bytes: &[u8]) -> Result<Self, ChipDumpError> {
        Ay::from_bytes(bytes)
    }
    fn from_reader(file: File) -> Result<Self, ChipDumpError> {
        Ay::from_reader(file)
    }
}

/// A source and its result code. A failed load leaves SoLoud's source
/// without a dump (reference patches 0085 and 0092): the default.
fn loaded<T: ChipSource>(result: Result<T, ChipDumpError>) -> (T, i32) {
    match result {
        Ok(source) => (source, SO_NO_ERROR),
        Err(ChipDumpError::Io(_)) => (T::default(), FILE_NOT_FOUND),
        Err(_) => (T::default(), FILE_LOAD_FAILED),
    }
}

/// `file` with `from` (`path`, default: `load`; `memory`: `loadMem`;
/// `file`: `loadFile` of a `DiskFile`), or no dump when no file is given.
fn load<T: ChipSource>(step: &Step) -> Result<(T, i32), ScenarioError> {
    if step.get("from").is_some() && step.get("file").is_none() {
        return Err(step.error("`from` needs `file`"));
    }
    if step.get("file").is_none() {
        return Ok((T::default(), SO_NO_ERROR));
    }
    let path = fixture_path(step)?;
    match step.get("from").unwrap_or("path") {
        "path" => Ok(loaded(T::open(&path))),
        "memory" => {
            let bytes = read_fixture(step, &path)?;
            // `loadMem` rejects empty memory.
            if bytes.is_empty() {
                return Ok((T::default(), INVALID_PARAMETER));
            }
            Ok(loaded(T::from_bytes(&bytes)))
        }
        "file" => Ok(loaded(T::from_reader(open_fixture(step, &path)?))),
        other => Err(step.error(format!("unknown from `{other}`"))),
    }
}

impl State {
    pub(crate) fn chip_op(&mut self, step: &Step) -> Result<(), ScenarioError> {
        check_generator_keys(step, &CHIP_KEYS)?;
        self.mixer(step)?;
        let code = match step.op.as_str() {
            "tedsid" => {
                let (tune, code) = load::<TedSid>(step)?;
                self.store_generator(step, Arc::new(tune))?;
                code
            }
            "ay" => {
                let (tune, code) = load::<Ay>(step)?;
                self.store_generator(step, Arc::new(tune))?;
                code
            }
            other => return Err(step.error(format!("unsupported operation `{other}`"))),
        };
        self.log(format!("{} {} {code}", step.op, step.text("name")?));
        Ok(())
    }
}
