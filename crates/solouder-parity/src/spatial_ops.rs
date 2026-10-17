//! Scenario operations on 3D audio: `sound3d`, `play3d`, `listener`,
//! `set3d`, `update3d` and `speaker`. Each mirrors the operation of the same
//! name in `reference/tests/parity_runner.cpp`.

use solouder_core::{Attenuation, Handle, PlayOptions};

use crate::bus_ops::Source;
use crate::ops::{code, hex32};
use crate::run::State;
use crate::scenario::{ScenarioError, Step};

/// Whether all of `keys` are given; some but not all is an error.
fn group(step: &Step, keys: &[&str], what: &str) -> Result<bool, ScenarioError> {
    let given = keys.iter().filter(|key| step.get(key).is_some()).count();
    if given != 0 && given != keys.len() {
        return Err(step.error(format!("{what} go together")));
    }
    Ok(given == keys.len())
}

/// Three numbers, 0 when absent.
fn triple(step: &Step, keys: [&str; 3]) -> Result<[f32; 3], ScenarioError> {
    Ok([
        step.number_or(keys[0], 0.0)?,
        step.number_or(keys[1], 0.0)?,
        step.number_or(keys[2], 0.0)?,
    ])
}

const XYZ: [&str; 3] = ["x", "y", "z"];

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
        } else if let Some($source) = $state.synths.get_mut($name) {
            $body
        }
    };
}

impl State {
    /// `AudioSource::set3dMinMaxDistance`, `set3dAttenuation` and
    /// `set3dListenerRelative`, in that order, on a sound, stream, bus or
    /// queue.
    pub(crate) fn sound3d(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["name", "min", "max", "model", "rolloff", "relative"])?;
        self.source(step, "name")?;
        let distance = group(step, &["min", "max"], "`min` and `max`")?;
        let model = group(step, &["model", "rolloff"], "`model` and `rolloff`")?;
        let index: u32 = step.value_or("model", 0)?;
        let relative = step.flag_or("relative", false)?;
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
            if step.get("relative").is_some() {
                source.set_3d_listener_relative(relative);
            }
        });
        Ok(())
    }

    /// `Soloud::play3d` with no velocity; `volume` defaults to 1 as in C++.
    pub(crate) fn play3d(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["sound", "as", "x", "y", "z", "volume", "paused", "via"])?;
        let source = self.source(step, "sound")?;
        let via = match step.get("via") {
            Some(_) => self.handle_named(step, "via")?,
            None => Handle::INVALID,
        };
        let name = step.text("as")?.to_owned();
        let position = triple(step, XYZ)?;
        let options = PlayOptions {
            volume: Some(step.number_or("volume", 1.0)?),
            paused: step.flag_or("paused", false)?,
            bus: via,
            ..PlayOptions::default()
        };
        let mixer = self.mixer(step)?;
        let handle = match &source {
            Source::Sound(sound) => mixer.play_3d(sound, position, options),
            Source::Stream(stream) => mixer.play_3d(stream, position, options),
            Source::Bus(bus) => mixer.play_3d(bus, position, options),
            Source::Queue(queue) => mixer.play_3d(queue, position, options),
            Source::Synth(synth) => mixer.play_3d(synth, position, options),
        };
        let sound_name = step.text("sound")?.to_owned();
        self.played.entry(sound_name).or_default().push(handle);
        self.handles.insert(name.clone(), handle);
        self.log(format!("play3d {name} {}", handle.raw()));
        Ok(())
    }

    /// `set3dListenerParameters` with `params=1`, otherwise
    /// `set3dListenerPosition`, `set3dListenerAt` and `set3dListenerUp` for
    /// the groups given.
    pub(crate) fn listener(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["x", "y", "z", "ax", "ay", "az", "ux", "uy", "uz", "params"])?;
        let has_position = group(step, &XYZ, "`x`, `y` and `z`")?;
        let has_at = group(step, &["ax", "ay", "az"], "`ax`, `ay` and `az`")?;
        let has_up = group(step, &["ux", "uy", "uz"], "`ux`, `uy` and `uz`")?;
        let params = step.flag_or("params", false)?;
        if params && !(has_position && has_at && has_up) {
            return Err(step.error("`params=1` needs the position, `at` and `up`"));
        }
        if !(has_position || has_at || has_up) {
            return Err(step.error("`listener` needs a position, `at` or `up`"));
        }
        let position = triple(step, XYZ)?;
        let at = triple(step, ["ax", "ay", "az"])?;
        let up = triple(step, ["ux", "uy", "uz"])?;
        let mixer = self.mixer(step)?;
        if params {
            mixer.set_3d_listener_parameters(position, at, up);
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
        Ok(())
    }

    /// `set3dSourcePosition` (or `set3dSourceParameters` with `params=1`),
    /// `set3dSourceMinMaxDistance` and `set3dSourceAttenuation`, in that
    /// order.
    pub(crate) fn set3d(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[
            "h", "x", "y", "z", "params", "min", "max", "model", "rolloff",
        ])?;
        let handle = self.handle(step)?;
        let has_position = group(step, &XYZ, "`x`, `y` and `z`")?;
        let distance = group(step, &["min", "max"], "`min` and `max`")?;
        let model = group(step, &["model", "rolloff"], "`model` and `rolloff`")?;
        let params = step.flag_or("params", false)?;
        if params && !has_position {
            return Err(step.error("`params=1` needs `x`, `y` and `z`"));
        }
        let index: u32 = step.value_or("model", 0)?;
        let position = triple(step, XYZ)?;
        let (min, max) = (step.number_or("min", 0.0)?, step.number_or("max", 0.0)?);
        let rolloff = step.number_or("rolloff", 0.0)?;
        let mixer = self.mixer(step)?;
        // `set3dSourceParameters` without velocities only moves the voice.
        if has_position {
            mixer.set_3d_source_position(handle, position);
        }
        if distance {
            mixer.set_3d_source_min_max_distance(handle, min, max);
        }
        if model {
            mixer.set_3d_source_attenuation(handle, Attenuation::from_index(index), rolloff);
        }
        Ok(())
    }

    pub(crate) fn update3d(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[])?;
        self.mixer(step)?.update_3d_audio();
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
