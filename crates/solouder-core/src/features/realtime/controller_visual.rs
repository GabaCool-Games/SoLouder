//! Visualization data through the controller: the mix's and buses' wave,
//! channel volumes and spectrum, as the renderer last published them. The
//! spectrum is computed here, on the calling thread.

use super::command::Command;
use super::controller::{Controller, ControllerError};
use crate::features::playback::{Bus, SoundId, VisualizationData, WAVE_LEN, spectrum};

/// The mix's visualization data ([`Mixer::wave`](crate::Mixer::wave)).
/// Each render that records it publishes a copy; turning recording off
/// keeps the last copy. Reading takes `&mut self` because it moves this
/// side to the latest copy.
impl Controller {
    /// Sets whether mixes record visualization data
    /// (`Soloud::setVisualizationEnable`); the mixer's configuration sets
    /// it at first.
    pub fn set_visualization(&mut self, enable: bool) -> Result<(), ControllerError> {
        self.send(Command::Visualization(enable))
    }

    /// The first [`WAVE_LEN`] frames of the latest recorded mix, summed over
    /// channels (`Soloud::getWave`).
    pub fn wave(&mut self) -> [f32; WAVE_LEN] {
        self.visualization_data().wave
    }

    /// The peak of `channel` over those frames
    /// (`Soloud::getApproximateVolume`); 0 for a channel the output does not
    /// have.
    pub fn approximate_volume(&mut self, channel: u32) -> f32 {
        if channel >= self.config.channels {
            return 0.0;
        }
        self.visualization_data().volumes[channel as usize]
    }

    /// [`Controller::wave`] and every channel's
    /// [`Controller::approximate_volume`], from the same render.
    pub fn visualization_data(&mut self) -> VisualizationData {
        let (wave, volumes) = self.visual.master();
        VisualizationData { wave, volumes }
    }

    /// Magnitudes of the first 256 bins of a 512-point FFT of
    /// [`Controller::wave`], zero-padded (`Soloud::calcFFT`).
    pub fn fft(&mut self) -> [f32; WAVE_LEN] {
        spectrum(&self.wave())
    }

    /// `bus`'s [`Mixer::bus_wave`](crate::Mixer::bus_wave) at the last
    /// render: `None` if the bus was not playing then, or this controller
    /// never enabled its visualization (see
    /// [`Controller::set_bus_visualization`]).
    pub fn bus_wave(&mut self, bus: &Bus) -> Option<[f32; WAVE_LEN]> {
        self.visual.bus(bus.id()).map(|(wave, _)| wave)
    }

    /// `bus`'s [`Mixer::bus_approximate_volume`](crate::Mixer::bus_approximate_volume)
    /// at the last render, as [`Controller::bus_wave`] reads it; 0 when
    /// that has no data.
    pub fn bus_approximate_volume(&mut self, bus: &Bus, channel: u32) -> f32 {
        if channel >= bus.channels() {
            return 0.0;
        }
        self.visual
            .bus(bus.id())
            .map_or(0.0, |(_, volumes)| volumes[channel as usize])
    }

    /// The spectrum of [`Controller::bus_wave`] (`Bus::calcFFT`).
    pub fn bus_fft(&mut self, bus: &Bus) -> Option<[f32; WAVE_LEN]> {
        self.bus_wave(bus).map(|wave| spectrum(&wave))
    }

    /// The command that has the renderer publish `bus`'s data, if it is
    /// not yet watched. Fails when every slot
    /// ([`RealtimeConfig::visualized_buses`](super::RealtimeConfig::visualized_buses))
    /// is taken.
    pub(super) fn bus_watch(
        &self,
        bus: Option<SoundId>,
    ) -> Result<Option<(SoundId, Command)>, ControllerError> {
        let Some(bus) = bus else { return Ok(None) };
        Ok(self.visual.watch(bus)?.map(|command| (bus, command)))
    }

    /// Sends a command from [`Controller::bus_watch`]; there must be room.
    pub(super) fn send_watch(&mut self, watch: Option<(SoundId, Command)>) {
        if let Some((bus, command)) = watch {
            let _ = self.send(command);
            self.visual.watched(bus);
        }
    }
}
