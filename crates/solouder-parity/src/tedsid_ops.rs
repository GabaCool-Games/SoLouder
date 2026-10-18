//! The scenario operation `tedsid`, which mirrors the operation of the
//! same name in `reference/tests/parity_generators.inc`.

use std::sync::Arc;

use solouder_sources::{ChipDumpError, TedSid};

use crate::files::{FILE_LOAD_FAILED, fixture_path, open_fixture, read_fixture};
use crate::generator_ops::check_generator_keys;
use crate::run::{SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

const TEDSID_KEYS: [&str; 3] = ["name", "file", "from"];

/// SoLoud's `INVALID_PARAMETER`.
const INVALID_PARAMETER: i32 = 1;
/// SoLoud's `FILE_NOT_FOUND`.
const FILE_NOT_FOUND: i32 = 2;

/// A tune and its result code. A failed load leaves SoLoud's `TedSid`
/// without a dump (reference patch 0078): `TedSid::new`.
fn tune(result: Result<TedSid, ChipDumpError>) -> (TedSid, i32) {
    match result {
        Ok(tune) => (tune, SO_NO_ERROR),
        Err(ChipDumpError::Io(_)) => (TedSid::new(), FILE_NOT_FOUND),
        Err(_) => (TedSid::new(), FILE_LOAD_FAILED),
    }
}

/// `file` with `from` (`path`, default: `load`; `memory`: `loadMem`;
/// `file`: `loadFile` of a `DiskFile`), or no dump when no file is given.
fn load_tedsid(step: &Step) -> Result<(TedSid, i32), ScenarioError> {
    if step.get("from").is_some() && step.get("file").is_none() {
        return Err(step.error("`from` needs `file`"));
    }
    if step.get("file").is_none() {
        return Ok((TedSid::new(), SO_NO_ERROR));
    }
    let path = fixture_path(step)?;
    match step.get("from").unwrap_or("path") {
        "path" => Ok(tune(TedSid::open(&path))),
        "memory" => {
            let bytes = read_fixture(step, &path)?;
            // `TedSid::loadMem` rejects empty memory.
            if bytes.is_empty() {
                return Ok((TedSid::new(), INVALID_PARAMETER));
            }
            Ok(tune(TedSid::from_bytes(&bytes)))
        }
        "file" => Ok(tune(TedSid::from_reader(open_fixture(step, &path)?))),
        other => Err(step.error(format!("unknown from `{other}`"))),
    }
}

impl State {
    pub(crate) fn tedsid_op(&mut self, step: &Step) -> Result<(), ScenarioError> {
        match step.op.as_str() {
            "tedsid" => {
                check_generator_keys(step, &TEDSID_KEYS)?;
                self.mixer(step)?;
                let (tune, code) = load_tedsid(step)?;
                self.store_generator(step, Arc::new(tune))?;
                self.log(format!("tedsid {} {code}", step.text("name")?));
                Ok(())
            }
            other => Err(step.error(format!("unsupported operation `{other}`"))),
        }
    }
}
