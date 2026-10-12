//! Scenario operations on sounds, voice groups and 16-bit output, and the
//! per-voice settings added to `set` in milestone D. Each mirrors the
//! operation of the same name in `reference/tests/parity_runner.cpp`.

use solouder_core::{Handle, SOLOUD_VERSION, Sound};

use crate::ops::{code, hex64};
use crate::run::{MAX_MIX_FRAMES, SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

impl State {
    /// The `set` settings after speed, in order: loop_point, auto_stop,
    /// tick/kill, abs_l/abs_r, channel/channel_volume, rate, seek.
    pub(crate) fn set_more(
        &mut self,
        step: &Step,
        name: &str,
        handle: Handle,
    ) -> Result<(), ScenarioError> {
        let present = |key: &str| step.get(key).is_some();
        let loop_point = step.seconds_or("loop_point", 0.0)?;
        let auto_stop = step.flag_or("auto_stop", true)?;
        let (tick, kill) = (step.flag_or("tick", false)?, step.flag_or("kill", false)?);
        let (left, right) = (step.number_or("abs_l", 0.0)?, step.number_or("abs_r", 0.0)?);
        let channel: u32 = step.value_or("channel", 0)?;
        let channel_volume = step.number_or("channel_volume", 0.0)?;
        let rate = step.number_or("rate", 0.0)?;
        let seek = step.seconds_or("seek", 0.0)?;

        let mixer = self.mixer(step)?;
        if present("loop_point") {
            mixer.set_loop_point(handle, loop_point);
        }
        if present("auto_stop") {
            mixer.set_auto_stop(handle, auto_stop);
        }
        if present("tick") {
            mixer.set_inaudible_behavior(handle, tick, kill);
        }
        if present("abs_l") {
            mixer.set_pan_absolute(handle, left, right);
        }
        if present("channel") {
            mixer.set_channel_volume(handle, channel, channel_volume);
        }
        if present("rate") {
            mixer.set_sample_rate(handle, rate);
        }
        if present("seek") {
            mixer.seek(handle, seek);
            self.log(format!("set {name} seek {SO_NO_ERROR}"));
        }
        Ok(())
    }

    /// Like `mix`, through the signed 16-bit output; samples are logged as
    /// their integer values.
    pub(crate) fn mix16(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["frames"])?;
        let frames: u32 = step.value("frames")?;
        if frames > MAX_MIX_FRAMES {
            return Err(step.error(format!("mix frames must be at most {MAX_MIX_FRAMES}")));
        }
        let mixer = self.mixer(step)?;
        let mut out = vec![0_i16; frames as usize * mixer.config().channels as usize];
        mixer.mix_interleaved_i16(&mut out);
        self.rendered
            .samples
            .extend(out.iter().map(|&s| f32::from(s)));
        Ok(())
    }

    fn named_sound(&self, step: &Step) -> Result<Sound, ScenarioError> {
        let name = step.text("name")?;
        self.sounds
            .get(name)
            .cloned()
            .ok_or_else(|| step.error(format!("no sound named `{name}`")))
    }

    pub(crate) fn sound_query(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["name"])?;
        let sound = self.named_sound(step)?;
        let count = self.mixer(step)?.count_sound(&sound);
        let name = step.text("name")?;
        self.log(format!(
            "sound_query {name} length={} count={count}",
            hex64(sound.length())
        ));
        Ok(())
    }

    /// `stopAudioSource`, or `AudioSource::stop` with `source=1`; both are
    /// [`solouder_core::Mixer::stop_sound`] here.
    pub(crate) fn stop_sound(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["name", "source"])?;
        let sound = self.named_sound(step)?;
        step.flag_or("source", false)?;
        self.mixer(step)?.stop_sound(&sound);
        Ok(())
    }

    pub(crate) fn group(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["as"])?;
        let name = step.text("as")?.to_owned();
        let group = self.mixer(step)?.create_voice_group();
        self.handles.insert(name.clone(), group);
        self.log(format!("group {name} {}", group.raw()));
        Ok(())
    }

    /// `group_add`, `group_destroy` and `group_query`.
    pub(crate) fn group_op(&mut self, step: &Step) -> Result<(), ScenarioError> {
        if step.op == "group_add" {
            step.check_keys(&["group", "h"])?;
        } else {
            step.check_keys(&["group"])?;
        }
        let name = step.text("group")?;
        let group = self
            .handles
            .get(name)
            .copied()
            .ok_or_else(|| step.error(format!("no handle named `{name}`")))?;
        let mut line = format!("{} {name}", step.op);
        match step.op.as_str() {
            "group_add" => {
                let voice = self.handle(step)?;
                let result = self.mixer(step)?.add_voice_to_group(group, voice);
                line += &format!(" {} {}", step.text("h")?, code(result));
            }
            "group_destroy" => {
                let result = self.mixer(step)?.destroy_voice_group(group);
                line += &format!(" {}", code(result));
            }
            _ => {
                let mixer = self.mixer(step)?;
                let is_group = mixer.is_voice_group(group);
                let empty = mixer.is_voice_group_empty(group);
                line += &format!(" is_group={} empty={}", u8::from(is_group), u8::from(empty));
            }
        }
        self.log(line);
        Ok(())
    }

    pub(crate) fn version(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[])?;
        self.mixer(step)?;
        self.log(format!("version {SOLOUD_VERSION}"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::run::run;
    use crate::scenario::parse;

    #[test]
    fn set_pairs_must_be_complete() {
        let text = "init\nsound name=a frames=4\nplay sound=a as=v\nset h=v tick=1\n";
        assert_eq!(run(&parse("x", text).unwrap()).unwrap_err().line, 4);
    }

    #[test]
    fn integer_sounds_need_amplitudes_in_range() {
        let text = "init\nsound name=a frames=4 bits=8 amp=2\n";
        assert_eq!(run(&parse("x", text).unwrap()).unwrap_err().line, 2);
        let text = "init\nsound name=a frames=4 bits=12\n";
        assert_eq!(run(&parse("x", text).unwrap()).unwrap_err().line, 2);
    }
}
