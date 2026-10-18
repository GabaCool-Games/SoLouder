//! Replays a scenario through the real-time [`Controller`] and [`Renderer`]
//! instead of a [`Mixer`](solouder_core::Mixer), so its samples can be
//! compared with [`run`](crate::run)'s: the real-time split must render what
//! the mixer renders.
//!
//! Sources are defined on a [`State`] of their own (its mixer never plays),
//! so stateful colliders count the replay's calls alone. Plays, voice and
//! global settings, fades, queued sounds and every 3D operation go through
//! the controller; streams are decoded by its [`StreamWorker`], serviced
//! before and after each render. Read-only operations are skipped, except
//! visualization reads (`visual`, `bus_wave`), which append what the
//! controller reads of the renderer's published data. The
//! replay stops at the first operation it cannot express, returning the
//! samples rendered so far: one the controller has no counterpart for, one
//! naming a voice it does not know (a group, except in `set3d`, which sets
//! each of the group's voices), a collider that reads voice handles (the
//! controller has none), a source replaced while it plays, or a request
//! the controller refuses for want of room (a full command queue).

use std::collections::{HashMap, HashSet};

use solouder_core::realtime::{RealtimeConfig, split};
use solouder_core::{
    Bus, Controller, ControllerError, Mixer, PlayOptions, Playable, Renderer, StreamWorker, VoiceId,
};

use crate::bus_ops::Source;
use crate::run::{MAX_MIX_FRAMES, State};
use crate::scenario::{Scenario, ScenarioError, Step};

/// What a replay rendered, and where it stopped.
#[derive(Debug)]
pub struct Replayed {
    /// Interleaved samples from every `mix` replayed.
    pub samples: Vec<f32>,
    /// The line and operation the replay stopped at, and why; `None` if it
    /// replayed the whole scenario.
    pub stopped: Option<(usize, String)>,
}

/// Replays `scenario` through the real-time controller and renderer.
pub fn replay_realtime(scenario: &Scenario) -> Result<Replayed, ScenarioError> {
    let mut replay = Replay {
        defs: State::new(),
        engine: None,
        ids: HashMap::new(),
        groups: HashMap::new(),
        sources: HashMap::new(),
        played: HashSet::new(),
        samples: Vec::new(),
    };
    for step in &scenario.steps {
        if let Some(reason) = replay.apply(step)? {
            return Ok(Replayed {
                samples: replay.samples,
                stopped: Some((step.line, format!("`{}`: {reason}", step.op))),
            });
        }
    }
    Ok(Replayed {
        samples: replay.samples,
        stopped: None,
    })
}

/// Why the replay stops, or `None` to go on.
pub(crate) type Outcome = Result<Option<String>, ScenarioError>;

pub(crate) fn stop(reason: impl Into<String>) -> Outcome {
    Ok(Some(reason.into()))
}

pub(crate) struct Engine {
    pub controller: Controller,
    pub renderer: Renderer,
    pub worker: StreamWorker,
    pub channels: usize,
}

pub(crate) struct Replay {
    pub defs: State,
    pub engine: Option<Engine>,
    /// Voices by scenario name; `None` for a play the controller refused.
    pub ids: HashMap<String, Option<VoiceId>>,
    /// Voice names by group name.
    pub groups: HashMap<String, Vec<String>>,
    /// The source each named voice plays.
    pub sources: HashMap<String, String>,
    /// Sources played, which may not be replaced.
    pub played: HashSet<String>,
    pub samples: Vec<f32>,
}

/// Room for everything a scenario queues between two mixes, applied at the
/// next render as the mixer applies it at once.
fn realtime_config() -> RealtimeConfig {
    RealtimeConfig {
        command_capacity: 4096,
        commands_per_render: 4096,
        retired_capacity: 4096,
        stream_chunks: 64,
        spatial_batches: 8,
        ..RealtimeConfig::default()
    }
}

impl Replay {
    fn apply(&mut self, step: &Step) -> Outcome {
        match step.op.as_str() {
            "tolerance" | "query" | "counts" | "sound_query" | "bus_query" | "queue_query"
            | "group_query" | "version" | "filter_info" | "state" | "samples" => Ok(None),
            "init" => self.init(step),
            "group" => {
                self.groups.insert(step.text("as")?.to_owned(), Vec::new());
                Ok(None)
            }
            "group_add" => {
                let voice = step.text("h")?.to_owned();
                if let Some(voices) = self.groups.get_mut(step.text("group")?) {
                    voices.push(voice);
                }
                Ok(None)
            }
            "sound" | "stream" | "load" | "bus" | "queue" => self.define(step, "name"),
            "filter" | "sound_filter" => self.define(step, ""),
            // Generators; their setters change what playing voices share.
            "noise" | "vic" | "sfxr" | "monotone" | "tedsid" | "ay" | "speech" | "vizsn" => {
                self.define(step, "name")
            }
            op if crate::generator_ops::is_generator_op(op) => self.define(step, ""),
            "sound3d" if step.get("collider") == Some("handle") => {
                stop("the controller gives colliders no handle")
            }
            "sound3d" => self.define(step, ""),
            "mix" => self.mix(step),
            "play" => self.play(step),
            "play3d" => self.play3d(step),
            "stop" => self.voice_op(step, |c, id| c.stop(id)),
            "stop_all" => self.engine_op(|c| c.stop_all()),
            "set" => self.set(step),
            "fade" => self.fade(step),
            "global" => self.global(step),
            "queue_add" => self.queue_add(step),
            "listener" => self.listener(step),
            "set3d" => self.set3d(step),
            "update3d" => self.update3d(),
            "sound_speed" => self.sound_speed(step),
            "speaker" => self.speaker(step),
            "visual" => self.visual(step),
            "bus_wave" => self.bus_wave(step),
            "bus_set" => self.bus_set(step),
            _ => stop("no real-time counterpart in the replay"),
        }
    }

    fn init(&mut self, step: &Step) -> Outcome {
        self.defs.apply(step)?;
        self.ids.clear();
        self.groups.clear();
        self.sources.clear();
        self.played.clear();
        self.engine = None;
        if let Some(mixer) = &self.defs.mixer {
            let config = mixer.config();
            let mixer = Mixer::new(config).expect("the same configuration as the State's");
            let (mut controller, renderer) = split(mixer, realtime_config());
            let worker = controller.take_stream_worker().expect("a new controller");
            self.engine = Some(Engine {
                controller,
                renderer,
                worker,
                channels: config.channels as usize,
            });
        }
        Ok(None)
    }

    /// A definition, on the State; `name_key` names what it may replace.
    fn define(&mut self, step: &Step, name_key: &str) -> Outcome {
        if !name_key.is_empty()
            && let Some(name) = step.get(name_key)
            && self.played.contains(name)
        {
            return stop("replaces a source that was played");
        }
        self.defs.apply(step)?;
        Ok(None)
    }

    pub(crate) fn engine(&mut self) -> Result<&mut Engine, Option<String>> {
        self.engine
            .as_mut()
            .ok_or_else(|| Some("no engine".to_owned()))
    }

    fn mix(&mut self, step: &Step) -> Outcome {
        let frames: u32 = step.value("frames")?;
        if frames > MAX_MIX_FRAMES {
            return Err(step.error("mix frames too large"));
        }
        let Some(engine) = self.engine.as_mut() else {
            return stop("no engine");
        };
        while engine.worker.service() {}
        let start = self.samples.len();
        let len = frames as usize * engine.channels;
        self.samples.resize(start + len, f32::NAN);
        engine.renderer.render(&mut self.samples[start..]);
        while engine.controller.poll_event().is_some() {}
        while engine.worker.service() {}
        Ok(None)
    }

    /// The voice `step`'s `key` names: `Err` stops the replay for a name it
    /// does not know; `Ok(None)` is a play the controller refused.
    pub(crate) fn id(&self, step: &Step, key: &str) -> Result<Option<VoiceId>, Option<String>> {
        let name = step.text(key).map_err(|e| Some(e.to_string()))?;
        self.ids
            .get(name)
            .copied()
            .ok_or_else(|| Some(format!("no voice named `{name}` (a group?)")))
    }

    fn voice_op(
        &mut self,
        step: &Step,
        op: impl FnOnce(&mut Controller, VoiceId) -> Result<(), ControllerError>,
    ) -> Outcome {
        let id = match self.id(step, "h") {
            Ok(id) => id,
            Err(reason) => return Ok(reason),
        };
        let engine = match self.engine() {
            Ok(engine) => engine,
            Err(reason) => return Ok(reason),
        };
        // An ended voice needs no care: the controller ignores commands for
        // it, as the mixer ignores a stale handle. A full queue stops.
        if let Some(id) = id
            && let Err(error) = op(&mut engine.controller, id)
        {
            return stop(error.to_string());
        }
        Ok(None)
    }

    pub(crate) fn engine_op(
        &mut self,
        op: impl FnOnce(&mut Controller) -> Result<(), ControllerError>,
    ) -> Outcome {
        match self.engine() {
            Ok(engine) => match op(&mut engine.controller) {
                Ok(()) => Ok(None),
                Err(error) => stop(format!("{error}")),
            },
            Err(reason) => Ok(reason),
        }
    }

    /// Records a play's voice under `as`; a full queue stops the replay.
    pub(crate) fn record(
        &mut self,
        step: &Step,
        played: Result<VoiceId, ControllerError>,
    ) -> Outcome {
        let id = match played {
            Ok(id) => Some(id),
            Err(error @ (ControllerError::QueueFull | ControllerError::StreamsFull)) => {
                return stop(format!("{error}"));
            }
            Err(_) => None,
        };
        let name = step.text("as")?.to_owned();
        let sound = step.text("sound")?.to_owned();
        self.played.insert(sound.clone());
        self.ids.insert(name.clone(), id);
        self.sources.insert(name, sound);
        Ok(None)
    }

    /// The bus `play`'s `bus`, or the bus whose voice `via` names.
    pub(crate) fn through(&self, step: &Step) -> Result<Option<Bus>, Option<String>> {
        let to_stop = |e: ScenarioError| Some(e.to_string());
        if step.get("bus").is_some() {
            return self.defs.bus_named(step, "bus").map(Some).map_err(to_stop);
        }
        let Some(via) = step.get("via") else {
            return Ok(None);
        };
        let Some(source) = self.sources.get(via) else {
            return Err(Some(format!("no voice named `{via}`")));
        };
        match self.defs.buses.get(source) {
            Some(bus) => Ok(Some(bus.clone())),
            None => Err(Some(format!("`{via}` is not a bus voice"))),
        }
    }

    fn play(&mut self, step: &Step) -> Outcome {
        if step.get("background").is_some() {
            return stop("background plays are not replayed");
        }
        let through = match self.through(step) {
            Ok(through) => through,
            Err(reason) => return Ok(reason),
        };
        let clock = step
            .get("clock")
            .map(|_| step.seconds_or("clock", 0.0))
            .transpose()?;
        let default = if through.is_some() { 1.0 } else { -1.0 };
        let volume = step.number_or("volume", default)?;
        let options = PlayOptions {
            volume: (volume >= 0.0).then_some(volume),
            pan: step.number_or("pan", 0.0)?,
            paused: step.flag_or("paused", false)?,
            ..PlayOptions::default()
        };
        let source = self.defs.source(step, "sound")?;
        let engine = match self.engine() {
            Ok(engine) => engine,
            Err(reason) => return Ok(reason),
        };
        let c = &mut engine.controller;
        let start = |c: &mut Controller, source: &dyn PlayAny| match (&through, clock) {
            (Some(bus), Some(time)) => source.bus_clocked(c, bus, time, options),
            (Some(bus), None) => source.bus(c, bus, options),
            (None, Some(time)) => source.clocked(c, time, options),
            (None, None) => source.now(c, options),
        };
        let played = with_source(&source, |s| start(c, s));
        self.record(step, played)
    }
}

/// Runs `f` on whichever kind of source `source` is.
pub(crate) fn with_source<R>(source: &Source, f: impl FnOnce(&dyn PlayAny) -> R) -> R {
    match source {
        Source::Sound(sound) => f(sound),
        Source::Stream(stream) => f(stream),
        Source::Bus(bus) => f(bus),
        Source::Queue(queue) => f(queue),
        Source::Synth(synth) => f(synth),
    }
}

type Played = Result<VoiceId, ControllerError>;

/// The controller's plays, for any source, as one object-safe trait.
pub(crate) trait PlayAny {
    fn now(&self, c: &mut Controller, options: PlayOptions) -> Played;
    fn clocked(&self, c: &mut Controller, time: f64, options: PlayOptions) -> Played;
    fn bus(&self, c: &mut Controller, bus: &Bus, options: PlayOptions) -> Played;
    fn bus_clocked(&self, c: &mut Controller, bus: &Bus, time: f64, o: PlayOptions) -> Played;
    fn play_3d(&self, c: &mut Controller, at: Placed<'_>, options: PlayOptions) -> Played;
}

/// Where a 3D play starts: position, velocity, and the bus and clock.
pub(crate) struct Placed<'a> {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub through: Option<&'a Bus>,
    pub clock: Option<f64>,
}

impl<T: Playable> PlayAny for T {
    fn now(&self, c: &mut Controller, options: PlayOptions) -> Played {
        c.play_with(self, options)
    }
    fn clocked(&self, c: &mut Controller, time: f64, options: PlayOptions) -> Played {
        c.play_clocked(time, self, options)
    }
    fn bus(&self, c: &mut Controller, bus: &Bus, options: PlayOptions) -> Played {
        c.bus_play(bus, self, options)
    }
    fn bus_clocked(&self, c: &mut Controller, bus: &Bus, time: f64, o: PlayOptions) -> Played {
        c.bus_play_clocked(bus, time, self, o)
    }
    fn play_3d(&self, c: &mut Controller, at: Placed<'_>, options: PlayOptions) -> Played {
        let (p, v) = (at.position, at.velocity);
        match (at.through, at.clock) {
            (Some(bus), Some(time)) => c.bus_play_3d_clocked(bus, time, self, p, v, options),
            (Some(bus), None) => c.bus_play_3d(bus, self, p, v, options),
            (None, Some(time)) => c.play_3d_clocked(time, self, p, v, options),
            (None, None) => c.play_3d(self, p, v, options),
        }
    }
}
