//! Scenario operations on fades, timers and engine settings. Each mirrors the
//! operation of the same name in `reference/tests/parity_runner.cpp`.

use solouder_core::Resampler;

use crate::ops::hex32;
use crate::run::State;
use crate::scenario::{ScenarioError, Step};

/// What `fade` and `oscillate` change.
#[derive(Clone, Copy)]
enum Param {
    Volume,
    Pan,
    Speed,
}

impl State {
    /// `fadeVolume`, `fadePan` and `fadeRelativePlaySpeed`, or without `h`
    /// `fadeGlobalVolume`.
    pub(crate) fn fade(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["h", "param", "to", "time"])?;
        let param = self.param(step)?;
        step.text("to")?;
        step.text("time")?;
        let to = step.number_or("to", 0.0)?;
        let time = step.seconds_or("time", 0.0)?;
        let handle = step.get("h").map(|_| self.handle(step)).transpose()?;
        let mixer = self.mixer(step)?;
        match (handle, param) {
            (None, _) => mixer.fade_global_volume(to, time),
            (Some(h), Param::Volume) => mixer.fade_volume(h, to, time),
            (Some(h), Param::Pan) => mixer.fade_pan(h, to, time),
            (Some(h), Param::Speed) => mixer.fade_relative_play_speed(h, to, time),
        }
        Ok(())
    }

    /// `oscillateVolume`, `oscillatePan` and `oscillateRelativePlaySpeed`, or
    /// without `h` `oscillateGlobalVolume`.
    pub(crate) fn oscillate(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["h", "param", "from", "to", "time"])?;
        let param = self.param(step)?;
        step.text("to")?;
        step.text("time")?;
        step.text("from")?;
        let from = step.number_or("from", 0.0)?;
        let to = step.number_or("to", 0.0)?;
        let time = step.seconds_or("time", 0.0)?;
        let handle = step.get("h").map(|_| self.handle(step)).transpose()?;
        let mixer = self.mixer(step)?;
        match (handle, param) {
            (None, _) => mixer.oscillate_global_volume(from, to, time),
            (Some(h), Param::Volume) => mixer.oscillate_volume(h, from, to, time),
            (Some(h), Param::Pan) => mixer.oscillate_pan(h, from, to, time),
            (Some(h), Param::Speed) => mixer.oscillate_relative_play_speed(h, from, to, time),
        }
        Ok(())
    }

    /// `schedulePause` then `scheduleStop`.
    pub(crate) fn schedule(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["h", "pause", "stop"])?;
        let handle = self.handle(step)?;
        if step.get("pause").is_none() && step.get("stop").is_none() {
            return Err(step.error("`schedule` needs `pause=` or `stop=`"));
        }
        let pause = step
            .get("pause")
            .map(|_| step.seconds_or("pause", 0.0))
            .transpose()?;
        let stop = step
            .get("stop")
            .map(|_| step.seconds_or("stop", 0.0))
            .transpose()?;
        let mixer = self.mixer(step)?;
        if let Some(time) = pause {
            mixer.schedule_pause(handle, time);
        }
        if let Some(time) = stop {
            mixer.schedule_stop(handle, time);
        }
        Ok(())
    }

    /// Logs the engine-wide settings.
    pub(crate) fn state(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[])?;
        let mixer = self.mixer(step)?;
        let resampler = match mixer.main_resampler() {
            Resampler::Point => 0,
            Resampler::Linear => 1,
            Resampler::CatmullRom => 2,
        };
        let line = format!(
            "state volume={} clip_scale={} resampler={resampler} max_active={}",
            hex32(mixer.global_volume()),
            hex32(mixer.post_clip_scaler()),
            mixer.max_active_voice_count()
        );
        self.log(line);
        Ok(())
    }

    /// `param`; without `h` only `volume` (the global volume) is accepted.
    fn param(&self, step: &Step) -> Result<Param, ScenarioError> {
        let param = match step.text("param")? {
            "volume" => Param::Volume,
            "pan" => Param::Pan,
            "speed" => Param::Speed,
            other => return Err(step.error(format!("unknown param `{other}`"))),
        };
        if step.get("h").is_none() && !matches!(param, Param::Volume) {
            return Err(step.error("without `h`, `param` must be `volume` (the global volume)"));
        }
        Ok(param)
    }
}
