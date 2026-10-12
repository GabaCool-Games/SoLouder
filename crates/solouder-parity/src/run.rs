//! Executes a scenario against `solouder-core`, mirroring what the C++ runner
//! does with the SoLoud C++ API.

use std::collections::HashMap;

use solouder_core::{Clipper, Handle, Mixer, MixerConfig, Sound};

use crate::scenario::{Scenario, ScenarioError, Step};

/// Largest `mix` a scenario may request. The C++ `Soloud::mix` writes past its
/// scratch buffer (at least 4096 frames) for larger requests.
pub const MAX_MIX_FRAMES: u32 = 4096;

/// SoLoud's `SO_NO_ERROR`.
pub(crate) const SO_NO_ERROR: i32 = 0;
/// SoLoud's `INVALID_PARAMETER`.
pub(crate) const INVALID_PARAMETER: i32 = 1;

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
        sounds: HashMap::new(),
        handles: HashMap::new(),
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

pub(crate) struct State {
    pub mixer: Option<Mixer>,
    /// Sounds by scenario name; cleared by `init`.
    pub sounds: HashMap<String, Sound>,
    /// Handles by scenario name; cleared by `init`.
    pub handles: HashMap<String, Handle>,
    pub rendered: Rendered,
}

impl State {
    fn apply(&mut self, step: &Step) -> Result<(), ScenarioError> {
        match step.op.as_str() {
            "tolerance" => self.tolerance(step),
            "init" => self.init(step),
            "mix" => self.mix(step),
            "sound" => self.sound(step),
            "load" => self.load(step),
            "samples" => self.samples(step),
            "play" => self.play(step),
            "stop" => self.stop(step),
            "stop_all" => self.stop_all(step),
            "set" => self.set(step),
            "global" => self.global(step),
            "query" => self.query(step),
            "counts" => self.counts(step),
            other => Err(step.error(format!("unsupported operation `{other}`"))),
        }
    }

    pub fn log(&mut self, line: String) {
        self.rendered.log.push(line);
    }

    /// The mixer, which every operation except `tolerance` and `init` needs.
    pub fn mixer(&mut self, step: &Step) -> Result<&mut Mixer, ScenarioError> {
        self.mixer
            .as_mut()
            .ok_or_else(|| step.error(format!("`{}` needs a successful init first", step.op)))
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
        step.check_keys(&["channels", "rate", "buffer", "clip"])?;
        let defaults = MixerConfig::default();
        // 0 means "automatic" in SoLoud's C API, as in `Soloud::init`.
        let auto = |value: u32, default: u32| if value == 0 { default } else { value };
        let clipper = match step.get("clip").unwrap_or("roundoff") {
            "roundoff" => Clipper::Roundoff,
            "hard" => Clipper::Hard,
            other => return Err(step.error(format!("unknown clip `{other}`"))),
        };
        let config = MixerConfig {
            channels: step.value_or("channels", defaults.channels)?,
            sample_rate: auto(step.value_or("rate", 0)?, defaults.sample_rate),
            buffer_size: auto(step.value_or("buffer", 0)?, defaults.buffer_size),
            clipper,
        };
        self.sounds.clear();
        self.handles.clear();
        self.mixer = None;
        let code = match Mixer::new(config) {
            Ok(mixer) => {
                self.mixer = Some(mixer);
                SO_NO_ERROR
            }
            Err(_) => INVALID_PARAMETER,
        };
        self.log(format!("init {code}"));
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
#[path = "run_tests.rs"]
mod tests;
