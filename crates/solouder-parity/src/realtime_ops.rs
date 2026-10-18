//! The replay's voice and global settings, fades, queued sounds and 3D
//! operations, each read as the operation of the same name reads its step.

use solouder_core::{Attenuation, Controller, ControllerError, PlayOptions, Resampler, VoiceId};

use crate::realtime::{Outcome, Placed, Replay, stop, with_source};
use crate::scenario::Step;
use crate::spatial_ops::{VELOCITY, XYZ, group, triple};

/// Keeps the first error of a setter that stops the replay: a full command
/// queue or a lost renderer, after which the replay would differ. Other
/// errors are values the mixer rejects too, and are ignored as it ignores
/// them.
fn note(failed: &mut Option<ControllerError>, result: Result<(), ControllerError>) {
    if let Err(error @ (ControllerError::QueueFull | ControllerError::Disconnected)) = result {
        failed.get_or_insert(error);
    }
}

/// The outcome of a step whose setters reported `failed`.
fn outcome(failed: Option<ControllerError>) -> Outcome {
    match failed {
        Some(error) => stop(error.to_string()),
        None => Ok(None),
    }
}

impl Replay {
    /// The controller and the voice `h` names, or what to return instead.
    fn voice(&mut self, step: &Step) -> Result<(&mut Controller, VoiceId), Outcome> {
        let id = match self.id(step, "h") {
            Ok(Some(id)) => id,
            Ok(None) => return Err(Ok(None)),
            Err(reason) => return Err(Ok(reason)),
        };
        match self.engine() {
            Ok(engine) => Ok((&mut engine.controller, id)),
            Err(reason) => Err(Ok(reason)),
        }
    }

    /// `set`, in the mixer's order.
    pub(crate) fn set(&mut self, step: &Step) -> Outcome {
        let has = |key: &str| step.get(key).is_some();
        let volume = step.number_or("volume", 0.0)?;
        let pan = step.number_or("pan", 0.0)?;
        let pause = step.flag_or("pause", false)?;
        let looping = step.flag_or("looping", false)?;
        let protect = step.flag_or("protect", false)?;
        let delay: u32 = step.value_or("delay", 0)?;
        let speed = step.number_or("speed", 0.0)?;
        let loop_point = step.seconds_or("loop_point", 0.0)?;
        let auto_stop = step.flag_or("auto_stop", true)?;
        let (tick, kill) = (step.flag_or("tick", false)?, step.flag_or("kill", false)?);
        let (left, right) = (step.number_or("abs_l", 0.0)?, step.number_or("abs_r", 0.0)?);
        let channel: u32 = step.value_or("channel", 0)?;
        let channel_volume = step.number_or("channel_volume", 0.0)?;
        let rate = step.number_or("rate", 0.0)?;
        let seek = step.seconds_or("seek", 0.0)?;
        let (c, id) = match self.voice(step) {
            Ok(voice) => voice,
            Err(outcome) => return outcome,
        };
        let mut failed = None;
        if has("volume") {
            note(&mut failed, c.set_volume(id, volume));
        }
        if has("pan") {
            note(&mut failed, c.set_pan(id, pan));
        }
        if has("pause") {
            note(&mut failed, c.set_pause(id, pause));
        }
        if has("looping") {
            note(&mut failed, c.set_looping(id, looping));
        }
        if has("protect") {
            note(&mut failed, c.set_protect(id, protect));
        }
        if has("delay") {
            note(&mut failed, c.set_delay_samples(id, delay));
        }
        if has("speed") {
            note(&mut failed, c.set_relative_play_speed(id, speed));
        }
        if has("loop_point") {
            note(&mut failed, c.set_loop_point(id, loop_point));
        }
        if has("auto_stop") {
            note(&mut failed, c.set_auto_stop(id, auto_stop));
        }
        if has("tick") {
            note(&mut failed, c.set_inaudible_behavior(id, tick, kill));
        }
        if has("abs_l") {
            note(&mut failed, c.set_pan_absolute(id, left, right));
        }
        if has("channel") {
            note(
                &mut failed,
                c.set_channel_volume(id, channel, channel_volume),
            );
        }
        if has("rate") {
            note(&mut failed, c.set_sample_rate(id, rate));
        }
        if has("seek") {
            note(&mut failed, c.seek(id, seek));
        }
        outcome(failed)
    }

    /// `fade` of a voice's volume, pan or speed, or of the global volume.
    pub(crate) fn fade(&mut self, step: &Step) -> Outcome {
        let to = step.number_or("to", 0.0)?;
        let time = step.seconds_or("time", 0.0)?;
        let param = step.text("param")?.to_owned();
        if step.get("h").is_none() {
            return self.engine_op(|c| c.fade_global_volume(to, time));
        }
        let (c, id) = match self.voice(step) {
            Ok(voice) => voice,
            Err(outcome) => return outcome,
        };
        let mut failed = None;
        note(
            &mut failed,
            match param.as_str() {
                "volume" => c.fade_volume(id, to, time),
                "pan" => c.fade_pan(id, to, time),
                _ => c.fade_relative_play_speed(id, to, time),
            },
        );
        outcome(failed)
    }

    /// `global`, in the mixer's order; `max_active` stops the replay (the
    /// controller cannot change the voice count).
    pub(crate) fn global(&mut self, step: &Step) -> Outcome {
        if step.get("max_active").is_some() {
            return stop("the controller cannot change the active voice count");
        }
        let volume = step.get("volume").map(|_| step.number_or("volume", 0.0));
        let scale = step
            .get("clip_scale")
            .map(|_| step.number_or("clip_scale", 0.0));
        let resampler = match step.get("resampler") {
            None => None,
            Some("point") => Some(Resampler::Point),
            Some("linear") => Some(Resampler::Linear),
            Some(_) => Some(Resampler::CatmullRom),
        };
        let pause = step
            .get("pause_all")
            .map(|_| step.flag_or("pause_all", false));
        let (volume, scale) = (volume.transpose()?, scale.transpose()?);
        let pause = pause.transpose()?;
        let visualization = step.get("visualization");
        let visualization = visualization.map(|_| step.flag_or("visualization", false));
        let visualization = visualization.transpose()?;
        self.engine_op(|c| {
            if let Some(volume) = volume {
                c.set_global_volume(volume)?;
            }
            if let Some(scale) = scale {
                c.set_post_clip_scaler(scale)?;
            }
            if let Some(resampler) = resampler {
                c.set_resampler(resampler)?;
            }
            if let Some(paused) = pause {
                c.set_pause_all(paused)?;
            }
            if let Some(enable) = visualization {
                c.set_visualization(enable)?;
            }
            Ok(())
        })
    }

    /// `queue_add`: `Queue::play` through the controller.
    pub(crate) fn queue_add(&mut self, step: &Step) -> Outcome {
        let queue = step.text("queue")?;
        let Some(queue) = self.defs.queues.get(queue).cloned() else {
            return Err(step.error(format!("no queue named `{queue}`")));
        };
        let name = step.text("sound")?;
        let sound = self.defs.sounds.get(name).cloned();
        let stream = self.defs.streams.get(name).cloned();
        self.engine_op(|c| {
            let queued = match (sound, stream) {
                (Some(sound), _) => c.queue_play(&queue, &sound),
                (None, Some(stream)) => c.queue_play(&queue, &stream),
                (None, None) => return Ok(()),
            };
            // A refused sound is logged by the mixer's run; only a full
            // command queue would make the replay differ.
            match queued {
                Err(solouder_core::ControllerError::QueueFull) => queued,
                _ => Ok(()),
            }
        })
    }

    pub(crate) fn play3d(&mut self, step: &Step) -> Outcome {
        let mut through = None;
        if step.get("bus").is_some() || step.get("via").is_some() {
            match self.through_bus(step) {
                Ok(bus) => through = Some(bus),
                Err(reason) => return Ok(reason),
            }
        }
        let clock = step
            .get("clock")
            .map(|_| step.seconds_or("clock", 0.0))
            .transpose()?;
        let options = PlayOptions {
            volume: Some(step.number_or("volume", 1.0)?),
            paused: step.flag_or("paused", false)?,
            ..PlayOptions::default()
        };
        let at = Placed {
            position: triple(step, XYZ)?,
            velocity: triple(step, VELOCITY)?,
            through: through.as_ref(),
            clock,
        };
        let source = self.defs.source(step, "sound")?;
        let engine = match self.engine() {
            Ok(engine) => engine,
            Err(reason) => return Ok(reason),
        };
        let c = &mut engine.controller;
        let played = with_source(&source, |s| s.play_3d(c, at, options));
        self.record(step, played)
    }

    fn through_bus(&self, step: &Step) -> Result<solouder_core::Bus, Option<String>> {
        match self.through(step) {
            Ok(Some(bus)) => Ok(bus),
            Ok(None) => Err(Some("no bus".to_owned())),
            Err(reason) => Err(reason),
        }
    }

    pub(crate) fn listener(&mut self, step: &Step) -> Outcome {
        let has_position = group(step, &XYZ, "`x`, `y` and `z`")?;
        let has_at = group(step, &["ax", "ay", "az"], "`ax`, `ay` and `az`")?;
        let has_up = group(step, &["ux", "uy", "uz"], "`ux`, `uy` and `uz`")?;
        let has_velocity = group(step, &VELOCITY, "`vx`, `vy` and `vz`")?;
        let params = step.flag_or("params", false)?;
        let position = triple(step, XYZ)?;
        let at = triple(step, ["ax", "ay", "az"])?;
        let up = triple(step, ["ux", "uy", "uz"])?;
        let velocity = triple(step, VELOCITY)?;
        self.engine_op(|c| {
            if params {
                c.set_3d_listener_parameters(position, at, up, velocity);
                return Ok(());
            }
            if has_position {
                c.set_3d_listener_position(position);
            }
            if has_at {
                c.set_3d_listener_at(at);
            }
            if has_up {
                c.set_3d_listener_up(up);
            }
            if has_velocity {
                c.set_3d_listener_velocity(velocity);
            }
            Ok(())
        })
    }

    pub(crate) fn set3d(&mut self, step: &Step) -> Outcome {
        let has_position = group(step, &XYZ, "`x`, `y` and `z`")?;
        let has_velocity = group(step, &VELOCITY, "`vx`, `vy` and `vz`")?;
        let distance = group(step, &["min", "max"], "`min` and `max`")?;
        let model = group(step, &["model", "rolloff"], "`model` and `rolloff`")?;
        let params = step.flag_or("params", false)?;
        let index: u32 = step.value_or("model", 0)?;
        let doppler = step.number_or("doppler", 1.0)?;
        let position = triple(step, XYZ)?;
        let velocity = triple(step, VELOCITY)?;
        let (min, max) = (step.number_or("min", 0.0)?, step.number_or("max", 0.0)?);
        let rolloff = step.number_or("rolloff", 0.0)?;
        let name = step.text("h")?;
        let ids: Vec<VoiceId> = match self.groups.get(name) {
            Some(voices) => voices
                .iter()
                .filter_map(|voice| self.ids.get(voice).copied().flatten())
                .collect(),
            None => match self.id(step, "h") {
                Ok(id) => id.into_iter().collect(),
                Err(reason) => return Ok(reason),
            },
        };
        let c = match self.engine() {
            Ok(engine) => &mut engine.controller,
            Err(reason) => return Ok(reason),
        };
        for id in ids {
            if params {
                c.set_3d_source_parameters(id, position, velocity);
            } else {
                if has_position {
                    c.set_3d_source_position(id, position);
                }
                if has_velocity {
                    c.set_3d_source_velocity(id, velocity);
                }
            }
            if distance {
                c.set_3d_source_min_max_distance(id, min, max);
            }
            if model {
                c.set_3d_source_attenuation(id, Attenuation::from_index(index), rolloff);
            }
            if step.get("doppler").is_some() {
                c.set_3d_source_doppler_factor(id, doppler);
            }
        }
        Ok(None)
    }

    pub(crate) fn update3d(&mut self) -> Outcome {
        self.engine_op(|c| c.update_3d_audio())
    }

    /// `sound_speed` with a speed; without one it only reads.
    pub(crate) fn sound_speed(&mut self, step: &Step) -> Outcome {
        if step.get("speed").is_none() {
            return Ok(None);
        }
        let speed = step.number_or("speed", 0.0)?;
        // The controller rejects the speeds the mixer rejects.
        self.engine_op(|c| {
            let _ = c.set_3d_sound_speed(speed);
            Ok(())
        })
    }

    /// `speaker` with a position; without one it only reads.
    pub(crate) fn speaker(&mut self, step: &Step) -> Outcome {
        if !group(step, &XYZ, "`x`, `y` and `z`")? {
            return Ok(None);
        }
        let channel: u32 = step.value("channel")?;
        let position = triple(step, XYZ)?;
        // The controller rejects the channels the mixer rejects.
        self.engine_op(|c| {
            let _ = c.set_speaker_position(channel, position);
            Ok(())
        })
    }
}
