// Ported from SoLoud src/core/soloud_bus.cpp and
// src/core/soloud_audiosource.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::control::ControlError;
use super::handle::Handle;
use super::mix_bus::{Layout, mix_bus};
use super::sound::SoundId;
use super::source::private::{Prepared, Sealed};
use super::source::{Playable, SourceSettings, source_settings_api};
use super::voice::{Voice, VoiceSource, flags};
use super::voices::{BLOCK_LEN, Voices};
use crate::contracts::{MAX_CHANNELS, SAMPLE_GRANULARITY};
use crate::dsp::{Resampler, c_int};

const BLOCK: usize = SAMPLE_GRANULARITY as usize;
const MAX: usize = MAX_CHANNELS as usize;
/// Frames of visualization data.
pub(crate) const WAVE_LEN: usize = 256;
/// A bus's sample rate before speed (`AudioSource::mBaseSamplerate`).
pub(crate) const BUS_RATE: f32 = 44100.0;

/// A mixing bus (`Bus`): voices played through it are mixed at the bus
/// voice's rate, and the bus voice plays that mix like a sound, with its own
/// volume, pan, speed and filters.
///
/// Start a bus with [`Mixer::play_with`], then play voices through it with
/// [`Mixer::bus_play`] or move voices to it with [`Mixer::annex_sound`]. A
/// bus has one voice per mixer: playing it again stops its earlier voice.
/// Bus voices are protected and keep playing when inaudible.
///
/// Cloning a bus keeps its [`SoundId`], so a clone names the same bus voice.
#[derive(Clone, Debug)]
pub struct Bus {
    id: SoundId,
    settings: SourceSettings,
    channels: u32,
    resampler: Resampler,
    visualization: bool,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus {
    /// A stereo bus with linear resampling and no visualization data.
    pub fn new() -> Self {
        Self {
            id: SoundId::next(),
            settings: SourceSettings::default(),
            channels: 2,
            resampler: Resampler::default(),
            visualization: false,
        }
    }

    /// Shared by this bus and its clones.
    pub fn id(&self) -> SoundId {
        self.id
    }

    source_settings_api!();

    /// Channels of the bus's mix.
    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// Sets the channels of bus voices started after this call
    /// (`Bus::setChannels`): 1, 2, 4, 6 or 8.
    pub fn set_channels(&mut self, channels: u32) -> Result<(), ControlError> {
        if !matches!(channels, 1 | 2 | 4 | 6 | 8) {
            return Err(ControlError::InvalidChannels);
        }
        self.channels = channels;
        Ok(())
    }

    /// How the bus resamples the voices playing through it.
    pub fn resampler(&self) -> Resampler {
        self.resampler
    }

    /// Sets the resampler of bus voices started after this call;
    /// [`Mixer::set_bus_resampler`] also changes a playing bus.
    pub fn set_resampler(&mut self, resampler: Resampler) {
        self.resampler = resampler;
    }

    /// Whether bus voices record the volume and wave data of their mix.
    pub fn visualization(&self) -> bool {
        self.visualization
    }

    /// Sets whether bus voices started after this call record visualization
    /// data; [`Mixer::set_bus_visualization`] also changes a playing bus.
    pub fn set_visualization(&mut self, enable: bool) {
        self.visualization = enable;
    }
}

impl Playable for Bus {
    fn source_id(&self) -> SoundId {
        self.id
    }
}

impl Sealed for Bus {
    fn prepare(&self) -> Prepared {
        let source = VoiceSource::Bus(Box::new(BusVoice::new(self)));
        let extra = flags::PROTECTED | flags::INAUDIBLE_TICK;
        Prepared {
            voice: Voice::new(&self.settings, self.id, source, BUS_RATE, extra),
            stop_others: true,
            default_volume: self.settings.volume,
        }
    }

    fn bus_id(&self) -> Option<SoundId> {
        Some(self.id)
    }
}

/// A bus voice's state (`BusInstance`).
#[derive(Debug)]
pub(crate) struct BusVoice {
    channels: usize,
    pub resampler: Resampler,
    pub visualization: bool,
    /// The bus voice's own handle, once a bus call has looked it up
    /// (`mChannelHandle`). Until then the bus mixes silence, and stopping it
    /// leaves the voices playing through it.
    pub handle: Option<Handle>,
    /// Each voice's resampled audio (`mScratch`).
    scratch: Vec<f32>,
    /// The latest block of the mix.
    pub block: Vec<f32>,
    /// The first frames of the latest mix, summed over channels
    /// (`mVisualizationWaveData`).
    pub wave: [f32; WAVE_LEN],
    /// Peak of each channel over those frames (`mVisualizationChannelVolume`).
    pub channel_volume: [f32; MAX],
}

impl BusVoice {
    fn new(bus: &Bus) -> Self {
        Self {
            channels: bus.channels as usize,
            resampler: bus.resampler,
            visualization: bus.visualization,
            handle: None,
            scratch: vec![0.0; BLOCK_LEN],
            block: vec![0.0; BLOCK_LEN],
            wave: [0.0; WAVE_LEN],
            channel_volume: [0.0; MAX],
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Mixes `frames` frames, at most one block, of the voices playing
    /// through the bus into `block` (`BusInstance::getAudio`).
    pub fn render(&mut self, voices: &mut Voices, sample_rate: f32, time: f64, frames: u32) {
        let Some(handle) = self.handle else {
            self.block[..BLOCK * self.channels].fill(0.0);
            return;
        };
        let layout = Layout {
            frames,
            stride: BLOCK,
            channels: self.channels,
            sample_rate,
            resampler: self.resampler,
            time,
            bus: handle,
        };
        mix_bus(voices, &mut self.block, &mut self.scratch, layout);
        if self.visualization {
            self.visualize(frames as usize);
        }
    }

    /// Repeats the mix when it is shorter than the wave data.
    fn visualize(&mut self, frames: usize) {
        self.channel_volume = [0.0; MAX];
        for (i, wave) in self.wave.iter_mut().enumerate() {
            let at = if frames >= WAVE_LEN { i } else { i % frames };
            let mut sum = 0.0;
            for (c, peak) in self.channel_volume[..self.channels].iter_mut().enumerate() {
                let sample = self.block[at + c * BLOCK];
                *peak = peak.max(sample.abs());
                sum += sample;
            }
            *wave = sum;
        }
    }
}

/// SoLoud's generic seek on a bus voice (`AudioSourceInstance::seek`). A bus
/// cannot rewind, so seeking back does nothing. Seeking forward mixes the
/// bus's voices over the frames skipped, in passes of `scratch_frames`
/// divided by the bus's channels, each mixed one block at a time (reference
/// patch 0046). False when seeking back.
pub(crate) fn seek(
    voice: &mut Voice,
    voices: &mut Voices,
    seconds: f64,
    scratch_frames: usize,
    time: f64,
) -> bool {
    let offset = seconds - voice.stream_position;
    if offset <= 0.0 {
        return false;
    }
    let VoiceSource::Bus(bus) = &mut voice.source else {
        return true;
    };
    let pass = (scratch_frames / bus.channels) as i32;
    let mut remaining = c_int((f64::from(voice.base_sample_rate) * offset).floor());
    while remaining > 0 {
        let frames = pass.min(remaining);
        if bus.handle.is_some() {
            let mut done = 0;
            while done < frames {
                let n = (frames - done).min(BLOCK as i32);
                bus.render(voices, voice.sample_rate, time, n as u32);
                done += n;
            }
        }
        remaining -= frames;
    }
    voice.stream_position = seconds;
    true
}

#[cfg(test)]
#[path = "bus_tests.rs"]
mod tests;
