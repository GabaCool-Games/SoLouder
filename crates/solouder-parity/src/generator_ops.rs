//! Scenario operations on generators (SoLoud's audio sources other than
//! `Wav`, `WavStream`, `Bus` and `Queue`). Each mirrors the operation of the
//! same name in `reference/tests/parity_generators.inc`.

use std::sync::Arc;

use solouder_core::{Generator, Synth};
use solouder_sources::{Noise, NoiseType, OCTAVES};

use crate::run::State;
use crate::scenario::{ScenarioError, Step};

/// The `AudioSource` settings every generator operation takes.
const SOURCE_SETTINGS: [&str; 7] = [
    "volume",
    "looping",
    "loop_point",
    "auto_stop",
    "single",
    "tick",
    "kill",
];

const NOISE_KEYS: [&str; 12] = [
    "name", "type", "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9",
];

pub(crate) fn is_generator_op(op: &str) -> bool {
    matches!(
        op,
        "noise"
            | "noise_set"
            | "vic"
            | "vic_set"
            | "vic_query"
            | "sfxr"
            | "sfxr_load"
            | "sfxr_reset"
            | "sfxr_query"
            | "monotone"
            | "monotone_set"
            | "monotone_query"
            | "tedsid"
    )
}

/// `check_keys` with the source settings allowed too.
pub(crate) fn check_generator_keys(step: &Step, allowed: &[&str]) -> Result<(), ScenarioError> {
    let all: Vec<&str> = allowed.iter().chain(&SOURCE_SETTINGS).copied().collect();
    step.check_keys(&all)
}

/// `setVolume`, `setLooping`, `setLoopPoint`, `setAutoStop`,
/// `setSingleInstance` and `setInaudibleBehavior`, at their defaults when
/// not given.
fn source_settings(step: &Step, synth: &mut Synth) -> Result<(), ScenarioError> {
    synth.set_volume(step.number_or("volume", 1.0)?);
    synth.set_looping(step.flag_or("looping", false)?);
    synth.set_loop_point(step.seconds_or("loop_point", 0.0)?);
    synth.set_auto_stop(step.flag_or("auto_stop", true)?);
    synth.set_single_instance(step.flag_or("single", false)?);
    let (tick, kill) = (step.flag_or("tick", false)?, step.flag_or("kill", false)?);
    synth.set_inaudible_behavior(tick, kill);
    Ok(())
}

/// `type` for `Noise::setType`, then `s0` to `s9` for
/// `Noise::setOctaveScale` (missing ones 0) if any is given.
fn apply_noise(step: &Step, noise: &Noise) -> Result<(), ScenarioError> {
    if step.get("type").is_some() {
        let index: u32 = step.value("type")?;
        noise.set_type(NoiseType::from_index(index as i32));
    }
    let keys = &NOISE_KEYS[2..];
    let mut scale = [0.0; OCTAVES];
    for (value, key) in scale.iter_mut().zip(keys) {
        *value = step.number_or(key, 0.0)?;
    }
    if keys.iter().any(|key| step.get(key).is_some()) {
        noise.set_octave_scale(scale);
    }
    Ok(())
}

impl State {
    pub(crate) fn generator_op(&mut self, step: &Step) -> Result<(), ScenarioError> {
        match step.op.as_str() {
            "noise" => {
                check_generator_keys(step, &NOISE_KEYS)?;
                self.mixer(step)?;
                let noise = Arc::new(Noise::new());
                apply_noise(step, &noise)?;
                self.store_generator(step, noise)
            }
            "noise_set" => {
                step.check_keys(&NOISE_KEYS)?;
                let noise = self.generator::<Noise>(step, "noise")?;
                apply_noise(step, &noise)
            }
            op if op.starts_with("vic") => self.vic_op(step),
            op if op.starts_with("sfxr") => self.sfxr_op(step),
            op if op.starts_with("monotone") => self.monotone_op(step),
            op if op.starts_with("tedsid") => self.tedsid_op(step),
            other => Err(step.error(format!("unsupported operation `{other}`"))),
        }
    }

    /// Registers a generator with the source settings of `step`. Replacing
    /// a name stops the old generator's voices, as deleting it does in C++.
    pub(crate) fn store_generator(
        &mut self,
        step: &Step,
        generator: Arc<dyn Generator>,
    ) -> Result<(), ScenarioError> {
        let name = step.text("name")?.to_owned();
        let mut synth = Synth::new(generator)
            .map_err(|error| step.error(format!("generator rejected: {error}")))?;
        source_settings(step, &mut synth)?;
        if let Some(old) = self.synths.insert(name, synth) {
            self.mixer(step)?.stop_sound(&old);
        }
        Ok(())
    }

    /// The generator named by `name`, which must be a `T`.
    pub(crate) fn generator<T: Generator + 'static>(
        &self,
        step: &Step,
        what: &str,
    ) -> Result<Arc<T>, ScenarioError> {
        let name = step.text("name")?;
        let generator = self
            .synths
            .get(name)
            .map(|synth| Arc::clone(synth.generator()));
        generator
            .and_then(|generator| downcast::<T>(generator))
            .ok_or_else(|| step.error(format!("no {what} named `{name}`")))
    }
}

fn downcast<T: Generator + 'static>(generator: Arc<dyn Generator>) -> Option<Arc<T>> {
    let any: Arc<dyn std::any::Any + Send + Sync> = generator;
    any.downcast::<T>().ok()
}
