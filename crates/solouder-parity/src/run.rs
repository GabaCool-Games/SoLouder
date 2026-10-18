//! Executes a scenario against `solouder-core`, mirroring what the C++ runner
//! does with the SoLoud C++ API.

use std::collections::HashMap;
use std::sync::Arc;

use solouder_core::filters::Filter;
use solouder_core::{Bus, Clipper, Handle, Mixer, MixerConfig, Queue, Sound, Stream, Synth};

use crate::scenario::{Scenario, ScenarioError, Step};

/// Largest `mix` a scenario may request. The C++ `Soloud::mix` writes past its
/// scratch buffer (at least 4096 frames) for larger requests.
pub const MAX_MIX_FRAMES: u32 = 4096;

/// SoLoud's `SO_NO_ERROR`.
pub(crate) const SO_NO_ERROR: i32 = 0;
/// SoLoud's `INVALID_PARAMETER`.
pub(crate) const INVALID_PARAMETER: i32 = 1;
/// SoLoud's `NOT_IMPLEMENTED`.
pub(crate) const NOT_IMPLEMENTED: i32 = 6;

/// What a scenario produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    /// Interleaved samples from every `mix`, in order.
    pub samples: Vec<f32>,
    /// One line per step that returns a value.
    pub log: Vec<String>,
    /// Maximum absolute sample difference allowed against the reference;
    /// 0 (the default) asks for identical bits.
    pub tolerance: f32,
}

/// Runs `scenario` and returns its samples and log.
pub fn run(scenario: &Scenario) -> Result<Rendered, ScenarioError> {
    let mut state = State {
        mixer: None,
        sounds: HashMap::new(),
        streams: HashMap::new(),
        buses: HashMap::new(),
        queues: HashMap::new(),
        synths: HashMap::new(),
        played: HashMap::new(),
        handles: HashMap::new(),
        filters: HashMap::new(),
        rendered: Rendered {
            samples: Vec::new(),
            log: Vec::new(),
            tolerance: 0.0,
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
    /// Streams by scenario name; cleared by `init`.
    pub streams: HashMap<String, Stream>,
    /// Buses by scenario name; cleared by `init`.
    pub buses: HashMap<String, Bus>,
    /// Queues by scenario name; cleared by `init`.
    pub queues: HashMap<String, Queue>,
    /// Generators by scenario name; cleared by `init`.
    pub synths: HashMap<String, Synth>,
    /// Voices started from each sound name, stopped when the name is
    /// reloaded; cleared by `init`.
    pub played: HashMap<String, Vec<Handle>>,
    /// Handles by scenario name; cleared by `init`.
    pub handles: HashMap<String, Handle>,
    /// Filters by scenario name; kept across `init`.
    pub filters: HashMap<String, Arc<dyn Filter>>,
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
            "stream" => self.stream(step),
            "samples" => self.samples(step),
            "play" => self.play(step),
            "stop" => self.stop(step),
            "stop_all" => self.stop_all(step),
            "set" => self.set(step),
            "global" => self.global(step),
            "query" => self.query(step),
            "voice_info" => self.voice_info(step),
            "counts" => self.counts(step),
            "fade" => self.fade(step),
            "oscillate" => self.oscillate(step),
            "schedule" => self.schedule(step),
            "state" => self.state(step),
            "mix16" => self.mix16(step),
            "sound_query" => self.sound_query(step),
            "stop_sound" => self.stop_sound(step),
            "group" => self.group(step),
            "group_add" | "group_destroy" | "group_query" => self.group_op(step),
            "version" => self.version(step),
            "filter" => self.filter(step),
            "sound_filter" => self.sound_filter(step),
            "global_filter" => self.global_filter(step),
            "filter_set" | "filter_fade" | "filter_oscillate" | "filter_query" => {
                self.filter_parameter(step)
            }
            "filter_info" => self.filter_info(step),
            "fft" => self.fft(step),
            "bus" => self.bus(step),
            "annex" => self.annex(step),
            "bus_query" => self.bus_query(step),
            "bus_wave" => self.bus_wave(step),
            "bus_filter" => self.bus_filter(step),
            "bus_set" => self.bus_set(step),
            "queue" => self.queue(step),
            "queue_add" => self.queue_add(step),
            "queue_query" => self.queue_query(step),
            "sound3d" => self.sound3d(step),
            "play3d" => self.play3d(step),
            "listener" => self.listener(step),
            "set3d" => self.set3d(step),
            "update3d" => self.update3d(step),
            "speaker" => self.speaker(step),
            op if crate::generator_ops::is_generator_op(op) => self.generator_op(step),
            other => Err(step.error(format!("unsupported operation `{other}`"))),
        }
    }

    /// Registers a loaded sound. Replacing a name stops the old sound's
    /// voices, as deleting the old `Wav` does in C++.
    pub fn store(&mut self, name: String, sound: Sound) {
        if let Some(handles) = self.played.remove(&name)
            && let Some(mixer) = self.mixer.as_mut()
        {
            for handle in handles {
                mixer.stop(handle);
            }
        }
        self.sounds.insert(name, sound);
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
        self.streams.clear();
        self.buses.clear();
        self.queues.clear();
        self.synths.clear();
        self.played.clear();
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
