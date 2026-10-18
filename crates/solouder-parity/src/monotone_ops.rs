//! Scenario operations on `Monotone`: `monotone`, `monotone_set` and
//! `monotone_query`. Each mirrors the operation of the same name in
//! `reference/tests/parity_generators.inc`.

use std::fmt::Write;
use std::sync::Arc;

use solouder_sources::{Monotone, MonotoneError};

use crate::files::{FILE_LOAD_FAILED, fixture_path, open_fixture, read_fixture};
use crate::generator_ops::check_generator_keys;
use crate::run::{SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

const MONOTONE_KEYS: [&str; 5] = ["name", "file", "from", "hardware", "waveform"];

/// SoLoud's `INVALID_PARAMETER`.
const INVALID_PARAMETER: i32 = 1;
/// SoLoud's `FILE_NOT_FOUND`.
const FILE_NOT_FOUND: i32 = 2;

/// A song and its result code. A failed load leaves SoLoud's `Monotone`
/// with an empty song (reference patch 0075): `Monotone::new`.
fn song(result: Result<Monotone, MonotoneError>) -> (Monotone, i32) {
    match result {
        Ok(song) => (song, SO_NO_ERROR),
        Err(MonotoneError::Io(_)) => (Monotone::new(), FILE_NOT_FOUND),
        Err(_) => (Monotone::new(), FILE_LOAD_FAILED),
    }
}

/// `file` with `from` (`path`, default: `load`; `memory`: `loadMem`;
/// `file`: `loadFile` of a `DiskFile`), or an empty song when no file is
/// given.
fn load_monotone(step: &Step) -> Result<(Monotone, i32), ScenarioError> {
    if step.get("from").is_some() && step.get("file").is_none() {
        return Err(step.error("`from` needs `file`"));
    }
    if step.get("file").is_none() {
        return Ok((Monotone::new(), SO_NO_ERROR));
    }
    let path = fixture_path(step)?;
    match step.get("from").unwrap_or("path") {
        "path" => Ok(song(Monotone::open(&path))),
        "memory" => {
            let bytes = read_fixture(step, &path)?;
            // `MemoryFile::openMem` rejects empty memory.
            if bytes.is_empty() {
                return Ok((Monotone::new(), INVALID_PARAMETER));
            }
            Ok(song(Monotone::from_bytes(&bytes)))
        }
        "file" => Ok(song(Monotone::from_reader(open_fixture(step, &path)?))),
        other => Err(step.error(format!("unknown from `{other}`"))),
    }
}

/// `hardware` and `waveform` (unsigned, passed on as SoLoud's `int`s; each
/// the current value when not given) for `set_params`, if either is given.
fn monotone_params(step: &Step, song: &Monotone) -> Result<i32, ScenarioError> {
    if step.get("hardware").is_none() && step.get("waveform").is_none() {
        return Ok(SO_NO_ERROR);
    }
    let hardware: u32 = step.value_or("hardware", song.hardware_channels())?;
    let waveform: u32 = step.value_or("waveform", song.waveform_raw() as u32)?;
    // An `int` of 0 or below is refused in SoLoud, as above 12 is here.
    Ok(match song.set_params_raw(hardware, waveform as i32) {
        Ok(()) => SO_NO_ERROR,
        Err(_) => INVALID_PARAMETER,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

impl State {
    pub(crate) fn monotone_op(&mut self, step: &Step) -> Result<(), ScenarioError> {
        match step.op.as_str() {
            "monotone" => {
                check_generator_keys(step, &MONOTONE_KEYS)?;
                self.mixer(step)?;
                let (song, code) = load_monotone(step)?;
                let params = monotone_params(step, &song)?;
                self.store_generator(step, Arc::new(song))?;
                self.log(format!("monotone {} {code} {params}", step.text("name")?));
                Ok(())
            }
            "monotone_set" => {
                step.check_keys(&["name", "hardware", "waveform"])?;
                let song = self.generator::<Monotone>(step, "monotone")?;
                let code = monotone_params(step, &song)?;
                self.log(format!("monotone_set {} {code}", step.text("name")?));
                Ok(())
            }
            "monotone_query" => {
                step.check_keys(&MONOTONE_KEYS[..1])?;
                let song = self.generator::<Monotone>(step, "monotone")?;
                let data = song.song();
                self.log(format!(
                    "monotone_query {} hardware={} waveform={} patterns={} tracks={} title={} comment={} order={}",
                    step.text("name")?,
                    song.hardware_channels(),
                    song.waveform_raw(),
                    data.patterns(),
                    data.tracks(),
                    hex(data.title()),
                    hex(data.comment()),
                    hex(data.order()),
                ));
                Ok(())
            }
            other => Err(step.error(format!("unsupported operation `{other}`"))),
        }
    }
}
