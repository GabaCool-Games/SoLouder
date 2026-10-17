//! Scenario operations on `Sfxr`: `sfxr`, `sfxr_load`, `sfxr_reset` and
//! `sfxr_query`. Each mirrors the operation of the same name in
//! `reference/tests/parity_generators.inc`.

use std::sync::Arc;

use solouder_sources::{Sfxr, SfxrError, SfxrPreset};

use crate::files::{FILE_LOAD_FAILED, fixture_path, open_fixture, read_fixture};
use crate::generator_ops::check_generator_keys;
use crate::ops::hex32;
use crate::run::{SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

const SFXR_KEYS: [&str; 5] = ["name", "preset", "seed", "file", "from"];

/// SoLoud's `INVALID_PARAMETER`.
const INVALID_PARAMETER: i32 = 1;
/// SoLoud's `FILE_NOT_FOUND`.
const FILE_NOT_FOUND: i32 = 2;

fn code(result: Result<(), SfxrError>) -> i32 {
    match result {
        Ok(()) => SO_NO_ERROR,
        Err(SfxrError::Io(_)) => FILE_NOT_FOUND,
        Err(_) => FILE_LOAD_FAILED,
    }
}

/// `preset` (with `seed`, default 0) for `Sfxr::loadPreset`, or `file`
/// with `from` (`path`, default: `loadParams`; `memory`: `loadParamsMem`;
/// `file`: `loadParamsFile` of a `DiskFile`). The result code, or
/// `SO_NO_ERROR` when neither is given.
fn load_sfxr(step: &Step, sfxr: &Sfxr) -> Result<i32, ScenarioError> {
    let present = |key: &str| step.get(key).is_some();
    if present("preset") && present("file") {
        return Err(step.error("`preset` and `file` cannot be combined"));
    }
    if present("seed") && !present("preset") {
        return Err(step.error("`seed` needs `preset`"));
    }
    if present("from") && !present("file") {
        return Err(step.error("`from` needs `file`"));
    }
    if present("preset") {
        let index: u32 = step.value("preset")?;
        let seed: u32 = step.value_or("seed", 0)?;
        // `loadPreset` takes an `int`: past 6 (or negative) it fails.
        return Ok(match SfxrPreset::from_index(index) {
            Some(preset) => {
                sfxr.load_preset(preset, seed as i32);
                SO_NO_ERROR
            }
            None => INVALID_PARAMETER,
        });
    }
    if !present("file") {
        return Ok(SO_NO_ERROR);
    }
    let path = fixture_path(step)?;
    match step.get("from").unwrap_or("path") {
        "path" => Ok(code(sfxr.load_params_file(&path))),
        "memory" => {
            let bytes = read_fixture(step, &path)?;
            // `MemoryFile::openMem` rejects empty memory.
            if bytes.is_empty() {
                return Ok(INVALID_PARAMETER);
            }
            Ok(code(sfxr.load_params(&bytes)))
        }
        "file" => Ok(code(sfxr.load_params_reader(open_fixture(step, &path)?))),
        other => Err(step.error(format!("unknown from `{other}`"))),
    }
}

impl State {
    pub(crate) fn sfxr_op(&mut self, step: &Step) -> Result<(), ScenarioError> {
        match step.op.as_str() {
            "sfxr" => {
                check_generator_keys(step, &SFXR_KEYS)?;
                self.mixer(step)?;
                let sfxr = Sfxr::new();
                let code = load_sfxr(step, &sfxr)?;
                self.store_generator(step, Arc::new(sfxr))?;
                self.log(format!("sfxr {} {code}", step.text("name")?));
                Ok(())
            }
            "sfxr_load" => {
                step.check_keys(&SFXR_KEYS)?;
                let sfxr = self.generator::<Sfxr>(step, "sfxr")?;
                let code = load_sfxr(step, &sfxr)?;
                self.log(format!("sfxr_load {} {code}", step.text("name")?));
                Ok(())
            }
            "sfxr_reset" => {
                step.check_keys(&SFXR_KEYS[..1])?;
                self.generator::<Sfxr>(step, "sfxr")?.reset_params();
                Ok(())
            }
            "sfxr_query" => {
                step.check_keys(&SFXR_KEYS[..1])?;
                let p = self.generator::<Sfxr>(step, "sfxr")?.params();
                let values = [
                    p.base_freq,
                    p.freq_limit,
                    p.freq_ramp,
                    p.freq_dramp,
                    p.duty,
                    p.duty_ramp,
                    p.vib_strength,
                    p.vib_speed,
                    p.vib_delay,
                    p.env_attack,
                    p.env_sustain,
                    p.env_decay,
                    p.env_punch,
                    p.lpf_resonance,
                    p.lpf_freq,
                    p.lpf_ramp,
                    p.hpf_freq,
                    p.hpf_ramp,
                    p.pha_offset,
                    p.pha_ramp,
                    p.repeat_speed,
                    p.arp_speed,
                    p.arp_mod,
                    p.master_vol,
                    p.sound_vol,
                ];
                let params: Vec<String> = values.into_iter().map(hex32).collect();
                self.log(format!(
                    "sfxr_query {} wave={} filter={} params={}",
                    step.text("name")?,
                    p.wave_type,
                    u8::from(p.filter_on),
                    params.join(",")
                ));
                Ok(())
            }
            other => Err(step.error(format!("unsupported operation `{other}`"))),
        }
    }
}
