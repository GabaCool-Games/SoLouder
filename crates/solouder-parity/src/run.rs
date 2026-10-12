//! Executes a scenario against `solouder-core`, mirroring what the C++ runner
//! does with the SoLoud C++ API.

use solouder_core::{Mixer, MixerConfig};

use crate::scenario::{Scenario, ScenarioError, Step};

/// Largest `mix` a scenario may request. The C++ `Soloud::mix` writes past its
/// scratch buffer (at least 4096 frames) for larger requests.
pub const MAX_MIX_FRAMES: u32 = 4096;

/// SoLoud's `SO_NO_ERROR`.
const SO_NO_ERROR: i32 = 0;
/// SoLoud's `INVALID_PARAMETER`.
const INVALID_PARAMETER: i32 = 1;

/// What a scenario produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    /// Interleaved samples from every `mix`, in order.
    pub samples: Vec<f32>,
    /// One line per step that returns a value.
    pub log: Vec<String>,
    /// Maximum absolute sample difference allowed against the reference.
    pub tolerance: f32,
}

/// Runs `scenario` and returns its samples and log.
pub fn run(scenario: &Scenario) -> Result<Rendered, ScenarioError> {
    let mut state = State {
        mixer: None,
        rendered: Rendered {
            samples: Vec::new(),
            log: Vec::new(),
            tolerance: 1e-6,
        },
    };
    for step in &scenario.steps {
        state.apply(step)?;
    }
    Ok(state.rendered)
}

struct State {
    mixer: Option<Mixer>,
    rendered: Rendered,
}

impl State {
    fn apply(&mut self, step: &Step) -> Result<(), ScenarioError> {
        match step.op.as_str() {
            "tolerance" => self.tolerance(step),
            "init" => self.init(step),
            "mix" => self.mix(step),
            other => Err(step.error(format!("unsupported operation `{other}`"))),
        }
    }

    fn tolerance(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["abs"])?;
        let abs: f32 = step.value("abs")?;
        if !(abs.is_finite() && abs >= 0.0) {
            return Err(step.error("tolerance must be a finite number >= 0"));
        }
        self.rendered.tolerance = abs;
        Ok(())
    }

    fn init(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["channels", "rate", "buffer"])?;
        let defaults = MixerConfig::default();
        // 0 means "automatic" in SoLoud's C API, as in `Soloud::init`.
        let auto = |value: u32, default: u32| if value == 0 { default } else { value };
        let config = MixerConfig {
            channels: step.value_or("channels", defaults.channels)?,
            sample_rate: auto(step.value_or("rate", 0)?, defaults.sample_rate),
            buffer_size: auto(step.value_or("buffer", 0)?, defaults.buffer_size),
        };
        self.mixer = None;
        let code = match Mixer::new(config) {
            Ok(mixer) => {
                self.mixer = Some(mixer);
                SO_NO_ERROR
            }
            Err(_) => INVALID_PARAMETER,
        };
        self.rendered.log.push(format!("init {code}"));
        Ok(())
    }

    fn mix(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["frames"])?;
        let frames: u32 = step.value("frames")?;
        if frames > MAX_MIX_FRAMES {
            return Err(step.error(format!("mix frames must be at most {MAX_MIX_FRAMES}")));
        }
        let mixer = self
            .mixer
            .as_mut()
            .ok_or_else(|| step.error("mix needs a successful init first"))?;
        let channels = mixer.config().channels as usize;
        let start = self.rendered.samples.len();
        self.rendered
            .samples
            .resize(start + frames as usize * channels, f32::NAN);
        mixer.mix_interleaved(&mut self.rendered.samples[start..]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::parse;

    fn run_text(text: &str) -> Result<Rendered, ScenarioError> {
        run(&parse("test", text).unwrap())
    }

    #[test]
    fn logs_init_results_and_mixes_requested_frames() {
        let rendered = run_text("init channels=3\ninit channels=6\nmix frames=5").unwrap();
        assert_eq!(rendered.log, ["init 1", "init 0"]);
        assert_eq!(rendered.samples, vec![0.0; 30]);
    }

    #[test]
    fn failed_init_drops_the_previous_engine() {
        let error = run_text("init\ninit buffer=100\nmix frames=1").unwrap_err();
        assert_eq!(error.line, 3);
    }

    #[test]
    fn rejects_unknown_operations_and_oversized_mixes() {
        assert!(run_text("play sound=x").is_err());
        assert!(run_text("init\nmix frames=4097").is_err());
        assert!(run_text("init volume=1").is_err());
    }

    #[test]
    fn tolerance_sets_the_comparison_limit() {
        assert_eq!(run_text("tolerance abs=0.5").unwrap().tolerance, 0.5);
        assert!(run_text("tolerance abs=-1").is_err());
    }
}
