//! Scenario operations on sounds and voices. Each mirrors the operation of
//! the same name in `reference/tests/parity_runner.cpp`.

use solouder_core::{Handle, Pcm, Resampler, Sound};

use crate::pattern::{MAX_SOUND_SAMPLES, Pattern};
use crate::run::{INVALID_PARAMETER, SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

pub(crate) fn code<E>(result: Result<(), E>) -> i32 {
    if result.is_ok() {
        SO_NO_ERROR
    } else {
        INVALID_PARAMETER
    }
}

pub(crate) fn hex32(value: f32) -> String {
    format!("{:08x}", value.to_bits())
}

pub(crate) fn hex64(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

impl State {
    pub(crate) fn handle(&self, step: &Step) -> Result<Handle, ScenarioError> {
        let name = step.text("h")?;
        self.handles
            .get(name)
            .copied()
            .ok_or_else(|| step.error(format!("no handle named `{name}`")))
    }

    pub(crate) fn sound(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[
            "name",
            "channels",
            "rate",
            "frames",
            "pattern",
            "amp",
            "period",
            "seed",
            "volume",
            "looping",
            "bits",
            "loop_point",
            "auto_stop",
            "single",
            "tick",
            "kill",
        ])?;
        self.mixer(step)?;
        let name = step.text("name")?.to_owned();
        let channels: u32 = step.value_or("channels", 1)?;
        let frames: u32 = step.value("frames")?;
        if u64::from(channels) * u64::from(frames) > u64::from(MAX_SOUND_SAMPLES) {
            return Err(step.error(format!(
                "channels * frames must be at most {MAX_SOUND_SAMPLES}"
            )));
        }
        let rate = step.number_or("rate", 44100.0)?;
        let amp = step.number_or("amp", 1.0)?;
        let pattern = Pattern::from_step(step)?;
        let volume = step.number_or("volume", 1.0)?;
        let looping = step.flag_or("looping", false)?;
        let bits: u32 = step.value_or("bits", 32)?;
        if bits != 32 && !(-1.0..=1.0).contains(&amp) {
            return Err(step.error("`bits` needs `amp` from -1 to 1"));
        }
        if ![8, 16, 32].contains(&bits) {
            return Err(step.error("`bits` must be 8, 16 or 32"));
        }
        let loop_point = step.seconds_or("loop_point", 0.0)?;
        let auto_stop = step.flag_or("auto_stop", true)?;
        let single = step.flag_or("single", false)?;
        let tick = step.flag_or("tick", false)?;
        let kill = step.flag_or("kill", false)?;

        let samples = pattern.render(channels, frames, amp);
        let pcm = match bits {
            8 => {
                let raw: Vec<u8> = samples
                    .iter()
                    .map(|&v| (v * 127.0 + 128.0) as i32 as u8)
                    .collect();
                Pcm::from_planar_u8(&raw, channels, rate)
            }
            16 => {
                let raw: Vec<i16> = samples.iter().map(|&v| (v * 32767.0) as i16).collect();
                Pcm::from_planar_i16(&raw, channels, rate)
            }
            _ => Pcm::from_planar(samples, channels, rate),
        };
        let result = pcm.map(|pcm| {
            let mut sound = Sound::new(pcm);
            sound.set_volume(volume);
            sound.set_looping(looping);
            sound.set_loop_point(loop_point);
            sound.set_auto_stop(auto_stop);
            sound.set_single_instance(single);
            sound.set_inaudible_behavior(tick, kill);
            self.store(name.clone(), sound);
        });
        self.log(format!("sound {name} {}", code(result)));
        Ok(())
    }

    pub(crate) fn stop(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["h"])?;
        let handle = self.handle(step)?;
        self.mixer(step)?.stop(handle);
        Ok(())
    }

    pub(crate) fn stop_all(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[])?;
        self.mixer(step)?.stop_all();
        Ok(())
    }

    /// Applies the given settings in a fixed order: volume, pan, pause,
    /// looping, protect, delay, speed, then those of [`State::set_more`].
    pub(crate) fn set(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[
            "h",
            "volume",
            "pan",
            "pause",
            "looping",
            "protect",
            "delay",
            "speed",
            "loop_point",
            "auto_stop",
            "tick",
            "kill",
            "abs_l",
            "abs_r",
            "channel",
            "channel_volume",
            "rate",
            "seek",
        ])?;
        for (a, b) in [
            ("tick", "kill"),
            ("abs_l", "abs_r"),
            ("channel", "channel_volume"),
        ] {
            if step.get(a).is_some() != step.get(b).is_some() {
                return Err(step.error(
                    "`tick`/`kill`, `abs_l`/`abs_r` and `channel`/`channel_volume` go in pairs",
                ));
            }
        }
        let name = step.text("h")?.to_owned();
        let handle = self.handle(step)?;
        let volume = step
            .get("volume")
            .map(|_| step.number_or("volume", 0.0))
            .transpose()?;
        let pan = step
            .get("pan")
            .map(|_| step.number_or("pan", 0.0))
            .transpose()?;
        let pause = step
            .get("pause")
            .map(|_| step.flag_or("pause", false))
            .transpose()?;
        let looping = step
            .get("looping")
            .map(|_| step.flag_or("looping", false))
            .transpose()?;
        let protect = step
            .get("protect")
            .map(|_| step.flag_or("protect", false))
            .transpose()?;
        let delay: Option<u32> = step.get("delay").map(|_| step.value("delay")).transpose()?;
        let speed = step
            .get("speed")
            .map(|_| step.number_or("speed", 0.0))
            .transpose()?;

        let mixer = self.mixer(step)?;
        if let Some(volume) = volume {
            mixer.set_volume(handle, volume);
        }
        if let Some(pan) = pan {
            mixer.set_pan(handle, pan);
        }
        if let Some(pause) = pause {
            mixer.set_pause(handle, pause);
        }
        if let Some(looping) = looping {
            mixer.set_looping(handle, looping);
        }
        if let Some(protect) = protect {
            mixer.set_protect_voice(handle, protect);
        }
        if let Some(delay) = delay {
            mixer.set_delay_samples(handle, delay);
        }
        if let Some(speed) = speed {
            let result = mixer.set_relative_play_speed(handle, speed);
            self.log(format!("set {name} speed {}", code(result)));
        }
        self.set_more(step, &name, handle)
    }

    /// Engine-wide settings, applied in order: volume, clip_scale, resampler,
    /// max_active, pause_all.
    pub(crate) fn global(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[
            "volume",
            "clip_scale",
            "resampler",
            "max_active",
            "pause_all",
            "visualization",
        ])?;
        let volume = step
            .get("volume")
            .map(|_| step.number_or("volume", 0.0))
            .transpose()?;
        let scale = step
            .get("clip_scale")
            .map(|_| step.number_or("clip_scale", 0.0))
            .transpose()?;
        let resampler = match step.get("resampler") {
            None => None,
            Some("point") => Some(Resampler::Point),
            Some("linear") => Some(Resampler::Linear),
            Some("catmullrom") => Some(Resampler::CatmullRom),
            Some(other) => return Err(step.error(format!("unknown resampler `{other}`"))),
        };
        let max_active: Option<u32> = step
            .get("max_active")
            .map(|_| step.value("max_active"))
            .transpose()?;

        let mixer = self.mixer(step)?;
        if let Some(volume) = volume {
            mixer.set_global_volume(volume);
        }
        if let Some(scale) = scale {
            mixer.set_post_clip_scaler(scale);
        }
        if let Some(resampler) = resampler {
            mixer.set_resampler(resampler);
        }
        if let Some(count) = max_active {
            let result = mixer.set_max_active_voice_count(count);
            self.log(format!("global max_active {}", code(result)));
        }
        if step.get("pause_all").is_some() {
            let paused = step.flag_or("pause_all", false)?;
            self.mixer(step)?.set_pause_all(paused);
        }
        if step.get("visualization").is_some() {
            let enable = step.flag_or("visualization", false)?;
            self.mixer(step)?.set_visualization(enable);
        }
        Ok(())
    }

    /// Logs a voice's state. Floats are logged as their IEEE bits; a stopped
    /// voice reports the C API's defaults.
    pub(crate) fn query(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["h"])?;
        let name = step.text("h")?.to_owned();
        let h = self.handle(step)?;
        let m = self.mixer(step)?;
        let flag = |value: Option<bool>| u8::from(value.unwrap_or(false));
        let line = format!(
            "query {name} valid={} paused={} looping={} protect={} loops={} volume={} \
             overall={} pan={} speed={} rate={} time={} pos={} loop_point={} auto_stop={} \
             info={}",
            u8::from(m.is_valid_voice_handle(h)),
            flag(m.is_paused(h)),
            flag(m.is_looping(h)),
            flag(m.is_protected(h)),
            m.loop_count(h).unwrap_or(0),
            hex32(m.volume(h).unwrap_or(0.0)),
            hex32(m.overall_volume(h).unwrap_or(0.0)),
            hex32(m.pan(h).unwrap_or(0.0)),
            hex32(m.relative_play_speed(h).unwrap_or(1.0)),
            hex32(m.sample_rate(h).unwrap_or(0.0)),
            hex64(m.voice_stream_time(h).unwrap_or(0.0)),
            hex64(m.stream_position(h).unwrap_or(0.0)),
            hex64(m.loop_point(h).unwrap_or(0.0)),
            flag(m.auto_stop(h)),
            hex32(m.info(h, 0).unwrap_or(0.0)),
        );
        self.log(line);
        Ok(())
    }

    pub(crate) fn counts(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[])?;
        let mixer = self.mixer(step)?;
        let line = format!(
            "counts voices={} active={}",
            mixer.voice_count(),
            mixer.active_voice_count()
        );
        self.log(line);
        Ok(())
    }
}
