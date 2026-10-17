//! Scenario operations on 3D audio: `sound3d`, `play3d`, `listener`,
//! `set3d`, `update3d`, `speaker` and `sound_speed`. Each mirrors the
//! operation of the same name in `reference/tests/parity_runner.cpp`.

use solouder_core::{Attenuation, Bus, Handle, Mixer, PlayOptions, Playable};

use crate::bus_ops::Source;
use crate::ops::{code, hex32};
use crate::run::State;
use crate::scenario::{ScenarioError, Step};
use crate::spatial_custom::{attenuator, collider};

/// Whether all of `keys` are given; some but not all is an error.
pub(crate) fn group(step: &Step, keys: &[&str], what: &str) -> Result<bool, ScenarioError> {
    let given = keys.iter().filter(|key| step.get(key).is_some()).count();
    if given != 0 && given != keys.len() {
        return Err(step.error(format!("{what} go together")));
    }
    Ok(given == keys.len())
}

/// Three numbers, 0 when absent.
pub(crate) fn triple(step: &Step, keys: [&str; 3]) -> Result<[f32; 3], ScenarioError> {
    Ok([
        step.number_or(keys[0], 0.0)?,
        step.number_or(keys[1], 0.0)?,
        step.number_or(keys[2], 0.0)?,
    ])
}

pub(crate) const XYZ: [&str; 3] = ["x", "y", "z"];
pub(crate) const VELOCITY: [&str; 3] = ["vx", "vy", "vz"];

/// Applies the `AudioSource` 3D setters to whichever kind of source it is.
macro_rules! with_source {
    ($state:expr, $name:expr, |$source:ident| $body:expr) => {
        if let Some($source) = $state.sounds.get_mut($name) {
            $body
        } else if let Some($source) = $state.streams.get_mut($name) {
            $body
        } else if let Some($source) = $state.buses.get_mut($name) {
            $body
        } else if let Some($source) = $state.queues.get_mut($name) {
            $body
        }
    };
}

/// How `play3d` starts a voice.
struct Start<'a> {
    through: Option<&'a Bus>,
    clock: Option<f64>,
    position: [f32; 3],
    velocity: [f32; 3],
    options: PlayOptions,
}

fn start(mixer: &mut Mixer, source: &impl Playable, how: &Start<'_>) -> Handle {
    let (p, v, options) = (how.position, how.velocity, how.options);
    match (how.through, how.clock) {
        (Some(bus), Some(time)) => mixer.bus_play_3d_clocked(bus, time, source, p, v, options),
        (Some(bus), None) => mixer.bus_play_3d(bus, source, p, v, options),
        (None, Some(time)) => mixer.play_3d_clocked(time, source, p, v, options),
        (None, None) => mixer.play_3d(source, p, v, options),
    }
}

impl State {
    /// `AudioSource::set3dMinMaxDistance`, `set3dAttenuation`,
    /// `set3dListenerRelative`, `set3dDopplerFactor`, `set3dDistanceDelay`,
    /// `set3dCollider` and `set3dAttenuator`, in that order, on a sound,
    /// stream, bus or queue.
    pub(crate) fn sound3d(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[
            "name",
            "min",
            "max",
            "model",
            "rolloff",
            "relative",
            "doppler",
            "delay",
            "collider",
            "data",
            "attenuator",
        ])?;
        self.source(step, "name")?;
        let distance = group(step, &["min", "max"], "`min` and `max`")?;
        let model = group(step, &["model", "rolloff"], "`model` and `rolloff`")?;
        let index: u32 = step.value_or("model", 0)?;
        let relative = step.flag_or("relative", false)?;
        let doppler = step.number_or("doppler", 1.0)?;
        let delay = step.flag_or("delay", false)?;
        let present = |key: &str| step.get(key).is_some();
        if present("data") && !present("collider") {
            return Err(step.error("`data` needs `collider`"));
        }
        let collider = present("collider").then(|| collider(step)).transpose()?;
        let data = step.value_or::<u32>("data", 0)? as i32;
        let attenuator = present("attenuator")
            .then(|| attenuator(step))
            .transpose()?;
        let (min, max) = (step.number_or("min", 0.0)?, step.number_or("max", 0.0)?);
        let rolloff = step.number_or("rolloff", 0.0)?;
        self.mixer(step)?;
        let name = step.text("name")?;
        with_source!(self, name, |source| {
            if distance {
                source.set_3d_min_max_distance(min, max);
            }
            if model {
                source.set_3d_attenuation(Attenuation::from_index(index), rolloff);
            }
            if present("relative") {
                source.set_3d_listener_relative(relative);
            }
            if present("doppler") {
                source.set_3d_doppler_factor(doppler);
            }
            if present("delay") {
                source.set_3d_distance_delay(delay);
            }
            if let Some(collider) = &collider {
                source.set_3d_collider(collider.clone(), data);
            }
            if let Some(attenuator) = &attenuator {
                source.set_3d_attenuator(attenuator.clone());
            }
        });
        Ok(())
    }

    /// `Soloud::play3d`, or `play3dClocked` with `clock`, or `Bus::play3d`
    /// and `Bus::play3dClocked` with `bus`; `volume` defaults to 1 as in C++.
    pub(crate) fn play3d(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[
            "sound", "as", "x", "y", "z", "vx", "vy", "vz", "volume", "paused", "via", "bus",
            "clock",
        ])?;
        let present = |key: &str| step.get(key).is_some();
        if present("clock") && present("paused") {
            return Err(step.error("`clock` and `paused` cannot be combined"));
        }
        if present("bus") && present("via") {
            return Err(step.error("`bus` and `via` cannot be combined"));
        }
        let source = self.source(step, "sound")?;
        let through = present("bus")
            .then(|| self.bus_named(step, "bus"))
            .transpose()?;
        let via = match step.get("via") {
            Some(_) => self.handle_named(step, "via")?,
            None => Handle::INVALID,
        };
        let name = step.text("as")?.to_owned();
        let how = Start {
            through: through.as_ref(),
            clock: present("clock")
                .then(|| step.seconds_or("clock", 0.0))
                .transpose()?,
            position: triple(step, XYZ)?,
            velocity: triple(step, VELOCITY)?,
            options: PlayOptions {
                volume: Some(step.number_or("volume", 1.0)?),
                paused: step.flag_or("paused", false)?,
                bus: via,
                ..PlayOptions::default()
            },
        };
        let mixer = self.mixer(step)?;
        let handle = match &source {
            Source::Sound(sound) => start(mixer, sound, &how),
            Source::Stream(stream) => start(mixer, stream, &how),
            Source::Bus(bus) => start(mixer, bus, &how),
            Source::Queue(queue) => start(mixer, queue, &how),
        };
        let sound_name = step.text("sound")?.to_owned();
        self.played.entry(sound_name).or_default().push(handle);
        self.handles.insert(name.clone(), handle);
        self.log(format!("play3d {name} {}", handle.raw()));
        Ok(())
    }

    /// `set3dListenerParameters` with `params=1` (velocity 0 unless given),
    /// otherwise `set3dListenerPosition`, `set3dListenerAt`,
    /// `set3dListenerUp` and `set3dListenerVelocity` for the groups given.
    pub(crate) fn listener(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[
            "x", "y", "z", "ax", "ay", "az", "ux", "uy", "uz", "vx", "vy", "vz", "params",
        ])?;
        let has_position = group(step, &XYZ, "`x`, `y` and `z`")?;
        let has_at = group(step, &["ax", "ay", "az"], "`ax`, `ay` and `az`")?;
        let has_up = group(step, &["ux", "uy", "uz"], "`ux`, `uy` and `uz`")?;
        let has_velocity = group(step, &VELOCITY, "`vx`, `vy` and `vz`")?;
        let params = step.flag_or("params", false)?;
        if params && !(has_position && has_at && has_up) {
            return Err(step.error("`params=1` needs the position, `at` and `up`"));
        }
        if !(has_position || has_at || has_up || has_velocity) {
            return Err(step.error("`listener` needs a position, `at`, `up` or a velocity"));
        }
        let position = triple(step, XYZ)?;
        let at = triple(step, ["ax", "ay", "az"])?;
        let up = triple(step, ["ux", "uy", "uz"])?;
        let velocity = triple(step, VELOCITY)?;
        let mixer = self.mixer(step)?;
        if params {
            mixer.set_3d_listener_parameters(position, at, up, velocity);
            return Ok(());
        }
        if has_position {
            mixer.set_3d_listener_position(position);
        }
        if has_at {
            mixer.set_3d_listener_at(at);
        }
        if has_up {
            mixer.set_3d_listener_up(up);
        }
        if has_velocity {
            mixer.set_3d_listener_velocity(velocity);
        }
        Ok(())
    }

    /// `set3dSourceParameters` with `params=1` (velocity 0 unless given),
    /// otherwise `set3dSourcePosition` and `set3dSourceVelocity`; then
    /// `set3dSourceMinMaxDistance`, `set3dSourceAttenuation` and
    /// `set3dSourceDopplerFactor`, in that order.
    pub(crate) fn set3d(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[
            "h", "x", "y", "z", "vx", "vy", "vz", "params", "min", "max", "model", "rolloff",
            "doppler",
        ])?;
        let handle = self.handle(step)?;
        let has_position = group(step, &XYZ, "`x`, `y` and `z`")?;
        let has_velocity = group(step, &VELOCITY, "`vx`, `vy` and `vz`")?;
        let distance = group(step, &["min", "max"], "`min` and `max`")?;
        let model = group(step, &["model", "rolloff"], "`model` and `rolloff`")?;
        let params = step.flag_or("params", false)?;
        if params && !has_position {
            return Err(step.error("`params=1` needs `x`, `y` and `z`"));
        }
        let index: u32 = step.value_or("model", 0)?;
        let doppler = step.number_or("doppler", 1.0)?;
        let position = triple(step, XYZ)?;
        let velocity = triple(step, VELOCITY)?;
        let (min, max) = (step.number_or("min", 0.0)?, step.number_or("max", 0.0)?);
        let rolloff = step.number_or("rolloff", 0.0)?;
        let mixer = self.mixer(step)?;
        if params {
            mixer.set_3d_source_parameters(handle, position, velocity);
        } else {
            if has_position {
                mixer.set_3d_source_position(handle, position);
            }
            if has_velocity {
                mixer.set_3d_source_velocity(handle, velocity);
            }
        }
        if distance {
            mixer.set_3d_source_min_max_distance(handle, min, max);
        }
        if model {
            mixer.set_3d_source_attenuation(handle, Attenuation::from_index(index), rolloff);
        }
        if step.get("doppler").is_some() {
            mixer.set_3d_source_doppler_factor(handle, doppler);
        }
        Ok(())
    }

    pub(crate) fn update3d(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[])?;
        self.mixer(step)?.update_3d_audio();
        Ok(())
    }

    /// `set3dSoundSpeed` when a speed is given, then `get3dSoundSpeed`.
    pub(crate) fn sound_speed(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["speed"])?;
        let speed = step
            .get("speed")
            .map(|_| step.number_or("speed", 0.0))
            .transpose()?;
        let mixer = self.mixer(step)?;
        let mut line = String::from("sound_speed");
        if let Some(speed) = speed {
            line += &format!(" set={}", code(mixer.set_3d_sound_speed(speed)));
        }
        line += &format!(" get={}", hex32(mixer.sound_speed_3d()));
        self.log(line);
        Ok(())
    }

    /// `setSpeakerPosition` when a position is given, then
    /// `getSpeakerPosition`.
    pub(crate) fn speaker(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["channel", "x", "y", "z"])?;
        let channel: u32 = step.value("channel")?;
        let set = group(step, &XYZ, "`x`, `y` and `z`")?;
        let position = triple(step, XYZ)?;
        let mixer = self.mixer(step)?;
        let mut line = format!("speaker {channel}");
        if set {
            let result = mixer.set_speaker_position(channel, position);
            line += &format!(" set={}", code(result));
        }
        let (result, [x, y, z]) = match mixer.speaker_position(channel) {
            Some(position) => (Ok(()), position),
            None => (Err(()), [0.0; 3]),
        };
        line += &format!(
            " get={} {} {} {}",
            code(result),
            hex32(x),
            hex32(y),
            hex32(z)
        );
        self.log(line);
        Ok(())
    }
}
