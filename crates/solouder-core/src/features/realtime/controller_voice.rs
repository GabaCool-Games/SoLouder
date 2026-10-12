//! Controller requests that change voices and the whole mix. Each mirrors
//! the [`Mixer`](crate::Mixer) call of the same name, applied at the start
//! of a later render; results the mixer would return are not reported.

use super::VoiceId;
use super::command::{Command, Faded, VoiceCommand};
use super::controller::{Controller, ControllerError};
use crate::dsp::Resampler;
use crate::features::playback::Playable;

impl Controller {
    fn voice(&mut self, id: VoiceId, command: VoiceCommand) -> Result<(), ControllerError> {
        self.send(Command::Voice(id, command))
    }

    /// Stops the voice; it reports [`Event::Finished`](super::Event) with
    /// [`FinishReason::Stopped`](super::FinishReason::Stopped), as do the
    /// voices playing through it if it is a bus.
    pub fn stop(&mut self, id: VoiceId) -> Result<(), ControllerError> {
        self.send(Command::Stop(id))
    }

    /// Stops every voice.
    pub fn stop_all(&mut self) -> Result<(), ControllerError> {
        self.send(Command::StopAll)
    }

    /// Stops every voice of `source` and its clones.
    pub fn stop_sound(&mut self, source: &impl Playable) -> Result<(), ControllerError> {
        self.send(Command::StopSound(source.source_id()))
    }

    /// Sets the voice's volume (ramped over the next render).
    pub fn set_volume(&mut self, id: VoiceId, volume: f32) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Volume(volume))
    }

    /// Sets the voice's stereo pan, -1 (left) to 1 (right).
    pub fn set_pan(&mut self, id: VoiceId, pan: f32) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Pan(pan))
    }

    /// Sets the left and right gains directly.
    pub fn set_pan_absolute(
        &mut self,
        id: VoiceId,
        left: f32,
        right: f32,
    ) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::PanAbsolute(left, right))
    }

    /// Sets the gain of one output speaker for the voice.
    pub fn set_channel_volume(
        &mut self,
        id: VoiceId,
        channel: u32,
        volume: f32,
    ) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::ChannelVolume(channel, volume))
    }

    /// Pauses or resumes the voice.
    pub fn set_pause(&mut self, id: VoiceId, paused: bool) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Pause(paused))
    }

    /// Pauses or resumes every voice.
    pub fn set_pause_all(&mut self, paused: bool) -> Result<(), ControllerError> {
        self.send(Command::PauseAll(paused))
    }

    /// Turns looping on or off for the voice.
    pub fn set_looping(&mut self, id: VoiceId, looping: bool) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Looping(looping))
    }

    /// Sets where the looping voice restarts, in seconds into its sound.
    pub fn set_loop_point(&mut self, id: VoiceId, seconds: f64) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::LoopPoint(seconds))
    }

    /// Sets whether the voice stops once its sound has played out.
    pub fn set_auto_stop(&mut self, id: VoiceId, auto_stop: bool) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::AutoStop(auto_stop))
    }

    /// Sets whether the voice keeps playing when inaudible, and whether it
    /// is stopped then.
    pub fn set_inaudible_behavior(
        &mut self,
        id: VoiceId,
        tick: bool,
        kill: bool,
    ) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Inaudible { tick, kill })
    }

    /// Protects the voice from being stolen or made inactive.
    pub fn set_protect(&mut self, id: VoiceId, protect: bool) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Protect(protect))
    }

    /// Delays the voice by `frames` output frames.
    pub fn set_delay_samples(&mut self, id: VoiceId, frames: u32) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Delay(frames))
    }

    /// Moves the voice to `seconds` into its sound. A bus or queue cannot
    /// seek back; such a seek does nothing.
    pub fn seek(&mut self, id: VoiceId, seconds: f64) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Seek(seconds))
    }

    /// Sets the voice's relative play speed, which must be greater than 0.
    pub fn set_relative_play_speed(
        &mut self,
        id: VoiceId,
        speed: f32,
    ) -> Result<(), ControllerError> {
        if speed.is_nan() || speed <= 0.0 {
            return Err(ControllerError::InvalidSpeed);
        }
        self.voice(id, VoiceCommand::Speed(speed))?;
        self.buses.set_speed(id, speed);
        Ok(())
    }

    /// Sets the rate the voice's sound plays at before speed.
    pub fn set_sample_rate(&mut self, id: VoiceId, rate: f32) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::SampleRate(rate))?;
        self.buses.set_sample_rate(id, rate);
        Ok(())
    }

    /// Fades the voice's volume to `to` over `seconds`.
    pub fn fade_volume(
        &mut self,
        id: VoiceId,
        to: f32,
        seconds: f64,
    ) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Fade(Faded::Volume, to, seconds))
    }

    /// Fades the voice's pan to `to` over `seconds`.
    pub fn fade_pan(&mut self, id: VoiceId, to: f32, seconds: f64) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Fade(Faded::Pan, to, seconds))
    }

    /// Fades the voice's relative play speed to `to` over `seconds`.
    pub fn fade_relative_play_speed(
        &mut self,
        id: VoiceId,
        to: f32,
        seconds: f64,
    ) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Fade(Faded::Speed, to, seconds))?;
        self.buses.raise_speed(id, to);
        Ok(())
    }

    /// Oscillates the voice's volume between `from` and `to`.
    pub fn oscillate_volume(
        &mut self,
        id: VoiceId,
        from: f32,
        to: f32,
        seconds: f64,
    ) -> Result<(), ControllerError> {
        self.voice(
            id,
            VoiceCommand::Oscillate(Faded::Volume, from, to, seconds),
        )
    }

    /// Oscillates the voice's pan between `from` and `to`.
    pub fn oscillate_pan(
        &mut self,
        id: VoiceId,
        from: f32,
        to: f32,
        seconds: f64,
    ) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Oscillate(Faded::Pan, from, to, seconds))
    }

    /// Oscillates the voice's relative play speed between `from` and `to`.
    pub fn oscillate_relative_play_speed(
        &mut self,
        id: VoiceId,
        from: f32,
        to: f32,
        seconds: f64,
    ) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::Oscillate(Faded::Speed, from, to, seconds))?;
        self.buses.raise_speed(id, from.max(to));
        Ok(())
    }

    /// Pauses the voice `seconds` from now.
    pub fn schedule_pause(&mut self, id: VoiceId, seconds: f64) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::SchedulePause(seconds))
    }

    /// Stops the voice `seconds` from now.
    pub fn schedule_stop(&mut self, id: VoiceId, seconds: f64) -> Result<(), ControllerError> {
        self.voice(id, VoiceCommand::ScheduleStop(seconds))
    }

    /// Sets the volume applied to the whole mix.
    pub fn set_global_volume(&mut self, volume: f32) -> Result<(), ControllerError> {
        self.send(Command::GlobalVolume(volume))
    }

    /// Fades the global volume to `to` over `seconds`.
    pub fn fade_global_volume(&mut self, to: f32, seconds: f64) -> Result<(), ControllerError> {
        self.send(Command::FadeGlobalVolume { to, seconds })
    }

    /// Oscillates the global volume between `from` and `to`.
    pub fn oscillate_global_volume(
        &mut self,
        from: f32,
        to: f32,
        seconds: f64,
    ) -> Result<(), ControllerError> {
        self.send(Command::OscillateGlobalVolume { from, to, seconds })
    }

    /// Sets the scale applied after clipping.
    pub fn set_post_clip_scaler(&mut self, scale: f32) -> Result<(), ControllerError> {
        self.send(Command::PostClipScaler(scale))
    }

    /// Selects the resampler for voices on the main output.
    pub fn set_resampler(&mut self, resampler: Resampler) -> Result<(), ControllerError> {
        self.send(Command::Resampler(resampler))
    }
}
