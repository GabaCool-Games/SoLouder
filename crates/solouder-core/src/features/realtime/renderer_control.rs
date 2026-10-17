//! Renderer handling of commands that change voices, filters and buses
//! without starting or stopping voices.

use super::command::{Command, Faded, ParamChange, VoiceCommand};
use super::renderer::Renderer;
use super::status::FinishReason;
use crate::features::playback::{FilterTarget, Handle, Mixer};

impl Renderer {
    pub(super) fn apply_control(&mut self, command: Command) {
        let mixer = &mut self.mixer;
        match command {
            Command::Voice(id, change) => {
                let handle = self.live.handle(id).unwrap_or(Handle::INVALID);
                let seek = matches!(change, VoiceCommand::Seek(_));
                apply_voice(mixer, handle, change);
                // A bus seek mixes: voices it plays to their end are gone
                // before a later command in this render can take their slot.
                if seek {
                    self.report_gone(FinishReason::Ended);
                }
            }
            Command::GlobalVolume(volume) => mixer.set_global_volume(volume),
            Command::FadeGlobalVolume { to, seconds } => mixer.fade_global_volume(to, seconds),
            Command::OscillateGlobalVolume { from, to, seconds } => {
                mixer.oscillate_global_volume(from, to, seconds);
            }
            Command::PostClipScaler(scale) => mixer.set_post_clip_scaler(scale),
            Command::Resampler(resampler) => mixer.set_resampler(resampler),
            Command::PauseAll(paused) => mixer.set_pause_all(paused),
            Command::GlobalFilter { slot, instance } => mixer.swap_global_filter(slot, instance),
            Command::FilterParameter {
                voice,
                slot,
                param,
                change,
            } => {
                let target = match voice {
                    None => FilterTarget::Global,
                    Some(id) => {
                        FilterTarget::Voice(self.live.handle(id).unwrap_or(Handle::INVALID))
                    }
                };
                apply_param(mixer, target, slot, param, change);
            }
            Command::BusFilter {
                bus,
                slot,
                instance,
            } => mixer.swap_bus_filter(bus, slot, instance),
            Command::BusResampler(bus, resampler) => mixer.set_bus_resampler_of(bus, resampler),
            Command::BusVisualization(bus, enable) => mixer.set_bus_visualization_of(bus, enable),
            Command::Annex { bus, voice } => {
                if let Some(handle) = self.live.handle(voice) {
                    mixer.annex_sound_of(bus, handle);
                }
            }
            Command::QueueStorage(queue, storage) => mixer.add_queue_storage(queue, storage),
            Command::Play(_)
            | Command::Stop(_)
            | Command::StopAll
            | Command::StopSound(_)
            | Command::QueueAdd { .. }
            | Command::Spatial(_) => {
                unreachable!("handled by Renderer::apply")
            }
        }
    }
}

fn apply_voice(mixer: &mut Mixer, handle: Handle, change: VoiceCommand) {
    match change {
        VoiceCommand::Volume(volume) => mixer.set_volume(handle, volume),
        VoiceCommand::Pan(pan) => mixer.set_pan(handle, pan),
        VoiceCommand::PanAbsolute(left, right) => mixer.set_pan_absolute(handle, left, right),
        VoiceCommand::ChannelVolume(channel, volume) => {
            mixer.set_channel_volume(handle, channel, volume);
        }
        VoiceCommand::Pause(paused) => mixer.set_pause(handle, paused),
        VoiceCommand::Looping(looping) => mixer.set_looping(handle, looping),
        VoiceCommand::LoopPoint(seconds) => mixer.set_loop_point(handle, seconds),
        VoiceCommand::AutoStop(auto_stop) => mixer.set_auto_stop(handle, auto_stop),
        VoiceCommand::Inaudible { tick, kill } => mixer.set_inaudible_behavior(handle, tick, kill),
        VoiceCommand::Protect(protect) => mixer.set_protect_voice(handle, protect),
        VoiceCommand::Delay(frames) => mixer.set_delay_samples(handle, frames),
        VoiceCommand::Seek(seconds) => {
            let _ = mixer.seek(handle, seconds);
        }
        VoiceCommand::Speed(speed) => {
            let _ = mixer.set_relative_play_speed(handle, speed);
        }
        VoiceCommand::SampleRate(rate) => mixer.set_sample_rate(handle, rate),
        VoiceCommand::Fade(what, to, seconds) => match what {
            Faded::Volume => mixer.fade_volume(handle, to, seconds),
            Faded::Pan => mixer.fade_pan(handle, to, seconds),
            Faded::Speed => mixer.fade_relative_play_speed(handle, to, seconds),
        },
        VoiceCommand::Oscillate(what, from, to, seconds) => match what {
            Faded::Volume => mixer.oscillate_volume(handle, from, to, seconds),
            Faded::Pan => mixer.oscillate_pan(handle, from, to, seconds),
            Faded::Speed => mixer.oscillate_relative_play_speed(handle, from, to, seconds),
        },
        VoiceCommand::SchedulePause(seconds) => mixer.schedule_pause(handle, seconds),
        VoiceCommand::ScheduleStop(seconds) => mixer.schedule_stop(handle, seconds),
    }
}

fn apply_param(
    mixer: &mut Mixer,
    target: FilterTarget,
    slot: u32,
    param: u32,
    change: ParamChange,
) {
    match change {
        ParamChange::Set(value) => mixer.set_filter_parameter(target, slot, param, value),
        ParamChange::Fade { to, seconds } => {
            mixer.fade_filter_parameter(target, slot, param, to, seconds);
        }
        ParamChange::Oscillate { from, to, seconds } => {
            mixer.oscillate_filter_parameter(target, slot, param, from, to, seconds);
        }
    }
}
