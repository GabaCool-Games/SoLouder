//! The layout of each bus voice the controller started, so filter instances
//! for a playing bus are prepared for the voice they will run on rather
//! than for the bus's current settings.

use super::VoiceId;
use crate::features::playback::SoundId;

/// A bus voice's channels, and the highest rate its filters may run at:
/// its sample rate times the fastest speed it was set, faded or
/// oscillated to.
#[derive(Clone, Copy, Debug)]
struct Layout {
    bus: SoundId,
    voice: VoiceId,
    channels: usize,
    sample_rate: f32,
    speed: f32,
}

/// One entry per bus voice, from its play request until its final event.
/// A bus can have several: a replay that has not started yet, or that will
/// fail, leaves the playing voice in place.
#[derive(Debug, Default)]
pub(super) struct BusLayouts(Vec<Layout>);

impl BusLayouts {
    /// Records a bus voice requested at `speed`.
    pub fn started(
        &mut self,
        bus: SoundId,
        voice: VoiceId,
        channels: usize,
        sample_rate: f32,
        speed: f32,
    ) {
        let layout = Layout {
            bus,
            voice,
            channels,
            sample_rate,
            speed,
        };
        self.0.push(layout);
    }

    /// The most channels and highest filter rate among `bus`'s voices
    /// requested by this controller and not finished, if any; whichever of
    /// them is playing when a filter arrives, it fits.
    pub fn of(&self, bus: SoundId) -> Option<(usize, f32)> {
        self.0
            .iter()
            .filter(|entry| entry.bus == bus)
            .map(|entry| (entry.channels, entry.sample_rate * entry.speed))
            .reduce(|(c1, r1), (c2, r2)| (c1.max(c2), r1.max(r2)))
    }

    /// The voice's speed is now `speed`.
    pub fn set_speed(&mut self, voice: VoiceId, speed: f32) {
        if let Some(entry) = self.find(voice) {
            entry.speed = speed;
        }
    }

    /// The voice's speed may reach `speed`.
    pub fn raise_speed(&mut self, voice: VoiceId, speed: f32) {
        if let Some(entry) = self.find(voice)
            && speed.is_finite()
        {
            entry.speed = entry.speed.max(speed);
        }
    }

    /// The voice's sample rate is now `rate`.
    pub fn set_sample_rate(&mut self, voice: VoiceId, rate: f32) {
        if let Some(entry) = self.find(voice)
            && rate.is_finite()
            && rate > 0.0
        {
            entry.sample_rate = rate;
        }
    }

    /// Forgets a voice that has finished or failed to start.
    pub fn finished(&mut self, voice: VoiceId) {
        self.0.retain(|entry| entry.voice != voice);
    }

    fn find(&mut self, voice: VoiceId) -> Option<&mut Layout> {
        self.0.iter_mut().find(|entry| entry.voice == voice)
    }
}
