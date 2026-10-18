//! Scenario operations on `Speech`: `speech`, `speech_set` and
//! `speech_query`. Each mirrors the operation of the same name in
//! `reference/tests/parity_generators.inc`.

use std::sync::Arc;

use solouder_sources::{KlattWaveform, Speech};

use crate::files::{fixture_path, read_fixture};
use crate::generator_ops::check_generator_keys;
use crate::ops::hex32;
use crate::run::{INVALID_PARAMETER, SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

const SPEECH_KEYS: [&str; 3] = ["name", "text", "file"];
const SET_KEYS: [&str; 5] = ["name", "frequency", "speed", "declination", "waveform"];

/// The text of `text` (`_` for spaces) or the bytes of the fixture `file`;
/// none without either.
fn text(step: &Step) -> Result<Option<Vec<u8>>, ScenarioError> {
    match (step.get("text"), step.get("file")) {
        (Some(_), Some(_)) => Err(step.error("`text` and `file` cannot be combined")),
        (Some(text), None) => Ok(Some(text.replace('_', " ").into_bytes())),
        (None, Some(_)) => read_fixture(step, &fixture_path(step)?).map(Some),
        (None, None) => Ok(None),
    }
}

impl State {
    pub(crate) fn speech_op(&mut self, step: &Step) -> Result<(), ScenarioError> {
        match step.op.as_str() {
            "speech" => {
                check_generator_keys(step, &SPEECH_KEYS)?;
                self.mixer(step)?;
                let text = text(step)?;
                let speech = Speech::new(text.as_deref().unwrap_or_default());
                self.store_generator(step, Arc::new(speech))?;
                self.log(format!("speech {} {SO_NO_ERROR}", step.text("name")?));
                Ok(())
            }
            "speech_set" => {
                step.check_keys(&SET_KEYS)?;
                let speech = self.generator::<Speech>(step, "speech")?;
                let mut params = speech.params();
                params.frequency = step.value_or("frequency", params.frequency)?;
                params.speed = step.number_or("speed", params.speed)?;
                params.declination = step.number_or("declination", params.declination)?;
                if step.get("waveform").is_some() {
                    params.waveform = KlattWaveform::from_index(step.value("waveform")?)
                        .ok_or_else(|| step.error("`waveform` must be 0 to 6"))?;
                }
                let code = match speech.set_params(params) {
                    Ok(()) => SO_NO_ERROR,
                    Err(_) => INVALID_PARAMETER,
                };
                self.log(format!("speech_set {} {code}", step.text("name")?));
                Ok(())
            }
            "speech_query" => {
                step.check_keys(&SET_KEYS[..1])?;
                let speech = self.generator::<Speech>(step, "speech")?;
                let p = speech.params();
                self.log(format!(
                    "speech_query {} frequency={} speed={} declination={} waveform={} frames={}",
                    step.text("name")?,
                    p.frequency,
                    hex32(p.speed),
                    hex32(p.declination),
                    p.waveform.index(),
                    speech.frames()
                ));
                Ok(())
            }
            other => Err(step.error(format!("unsupported operation `{other}`"))),
        }
    }
}
