// Ported from SoLoud src/core/soloud.cpp and src/core/soloud_bus.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Visualization data: the first frames of a mix summed over channels, the
//! peak of each channel over them, and their spectrum.

use super::Mixer;
use crate::contracts::MAX_CHANNELS;
use crate::dsp::fft::fft;

const MAX: usize = MAX_CHANNELS as usize;

/// Frames of visualization data: the length of [`Mixer::wave`] and
/// [`Mixer::fft`](super::Mixer::fft).
///
/// [`Mixer::wave`]: super::Mixer::wave
pub const WAVE_LEN: usize = 256;

/// One mix's visualization data, as [`Mixer::visualization_data`] and
/// [`Controller::visualization_data`](crate::Controller::visualization_data)
/// return it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisualizationData {
    /// The first [`WAVE_LEN`] frames, summed over channels (`getWave`).
    pub wave: [f32; WAVE_LEN],
    /// Each channel's peak over those frames (`getApproximateVolume`); 0
    /// past the output's channels.
    pub volumes: [f32; MAX],
}

impl VisualizationData {
    /// Magnitudes of the first [`WAVE_LEN`] bins of a 512-point FFT of
    /// [`wave`](Self::wave), zero-padded (`calcFFT`).
    pub fn spectrum(&self) -> [f32; WAVE_LEN] {
        spectrum(&self.wave)
    }
}

/// The whole mix's visualization data (`mVisualizationWaveData`,
/// `mVisualizationChannelVolume`) and whether it is recorded
/// (`ENABLE_VISUALIZATION`).
#[derive(Debug)]
pub(crate) struct Visualization {
    pub enabled: bool,
    pub wave: [f32; WAVE_LEN],
    pub volumes: [f32; MAX],
}

impl Visualization {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            wave: [0.0; WAVE_LEN],
            volumes: [0.0; MAX],
        }
    }
}

/// Records `frames` frames of a planar mix: each of the first
/// [`WAVE_LEN`] frames summed over `channels`, and each channel's peak over
/// them. A mix shorter than that is repeated. A mix of no frames keeps the
/// last data (reference patch 0068).
pub(crate) fn capture(
    wave: &mut [f32; WAVE_LEN],
    volumes: &mut [f32; MAX],
    planar: &[f32],
    frames: usize,
    stride: usize,
    channels: usize,
) {
    if frames == 0 {
        return;
    }
    *volumes = [0.0; MAX];
    for (i, sum) in wave.iter_mut().enumerate() {
        let at = if frames >= WAVE_LEN { i } else { i % frames };
        *sum = 0.0;
        for (c, peak) in volumes[..channels].iter_mut().enumerate() {
            let sample = planar[at + c * stride];
            // `if (vol < absvol) vol = absvol`: a NaN sample leaves the peak.
            *peak = peak.max(sample.abs());
            *sum += sample;
        }
    }
}

/// Magnitudes of the first [`WAVE_LEN`] bins of a 512-point FFT of `wave`,
/// zero-padded (`calcFFT`).
pub(crate) fn spectrum(wave: &[f32; WAVE_LEN]) -> [f32; WAVE_LEN] {
    let mut temp = [0.0_f32; 4 * WAVE_LEN];
    for (pair, &sample) in temp.as_chunks_mut::<2>().0.iter_mut().zip(wave) {
        pair[0] = sample;
    }
    fft(&mut temp);
    std::array::from_fn(|i| {
        let (real, imag) = (temp[2 * i], temp[2 * i + 1]);
        (real * real + imag * imag).sqrt()
    })
}

/// The whole mix's visualization data. Each mix, after clipping, records
/// its first [`WAVE_LEN`] frames (repeating a shorter mix); turning
/// recording off keeps the last data.
impl Mixer {
    /// Whether mixes record visualization data.
    pub fn visualization(&self) -> bool {
        self.visualization.enabled
    }

    /// Sets whether mixes record visualization data
    /// (`Soloud::setVisualizationEnable`).
    pub fn set_visualization(&mut self, enable: bool) {
        self.visualization.enabled = enable;
    }

    /// The first [`WAVE_LEN`] frames of the latest recorded mix, summed over
    /// channels (`Soloud::getWave`).
    pub fn wave(&self) -> [f32; WAVE_LEN] {
        self.visualization.wave
    }

    /// The peak of `channel` over those frames
    /// (`Soloud::getApproximateVolume`); 0 for a channel the output does not
    /// have (reference patch 0067).
    pub fn approximate_volume(&self, channel: u32) -> f32 {
        if channel >= self.config.channels {
            return 0.0;
        }
        self.visualization.volumes[channel as usize]
    }

    /// Magnitudes of the first 256 bins of a 512-point FFT of
    /// [`Mixer::wave`], zero-padded (`Soloud::calcFFT`).
    pub fn fft(&self) -> [f32; WAVE_LEN] {
        spectrum(&self.visualization.wave)
    }

    /// [`Mixer::wave`] and every channel's [`Mixer::approximate_volume`]
    /// together.
    pub fn visualization_data(&self) -> VisualizationData {
        VisualizationData {
            wave: self.visualization.wave,
            volumes: self.visualization.volumes,
        }
    }

    /// The wave and every channel's volume, for the real-time renderer.
    pub(crate) fn visualization_parts(&self) -> (&[f32; WAVE_LEN], &[f32; MAX]) {
        (&self.visualization.wave, &self.visualization.volumes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MixerConfig, Pcm, Sound};

    fn mixer(visualization: bool) -> Mixer {
        let config = MixerConfig {
            channels: 8,
            visualization,
            ..MixerConfig::default()
        };
        let mut mixer = Mixer::new(config).unwrap();
        let pcm = Pcm::from_planar(vec![0.5; 8 * 4096], 8, 44100.0).unwrap();
        mixer.play(&Sound::new(pcm));
        mixer
    }

    #[test]
    fn the_init_flag_and_the_setter_switch_recording() {
        let mut out = vec![0.0; 8 * 512];
        let mut off = mixer(false);
        off.mix_interleaved(&mut out);
        assert_eq!(off.wave(), [0.0; WAVE_LEN]);
        off.set_visualization(true);
        off.mix_interleaved(&mut out);
        let mut on = mixer(true);
        on.mix_interleaved(&mut out);
        on.mix_interleaved(&mut out);
        assert_eq!(on.wave(), off.wave());
        assert!(on.approximate_volume(7) > 0.0);
        assert_eq!(on.approximate_volume(8), 0.0);
        // Off keeps the last data.
        let (wave, volume) = (on.wave(), on.approximate_volume(0));
        on.set_visualization(false);
        on.mix_interleaved(&mut out);
        assert_eq!((on.wave(), on.approximate_volume(0)), (wave, volume));
        let sum: f32 = wave.iter().sum();
        assert!((on.fft()[0] - sum.abs()).abs() < 1e-3 * sum.abs());
    }

    #[test]
    fn short_mixes_repeat_and_empty_mixes_keep_the_data() {
        let (mut wave, mut volumes) = ([0.0; WAVE_LEN], [0.0; MAX]);
        // Two channels of three frames, stride 16.
        let mut planar = vec![0.0; 32];
        planar[..3].copy_from_slice(&[0.5, -1.0, 0.25]);
        planar[16..19].copy_from_slice(&[0.125, 0.0, -0.75]);
        capture(&mut wave, &mut volumes, &planar, 3, 16, 2);
        assert_eq!(wave[..4], [0.625, -1.0, -0.5, 0.625]);
        assert_eq!(wave[255], wave[0]);
        assert_eq!(volumes[..3], [1.0, 0.75, 0.0]);
        capture(&mut wave, &mut volumes, &[], 0, 0, 2);
        assert_eq!(volumes[..2], [1.0, 0.75]);
        assert_eq!(wave[1], -1.0);
    }

    #[test]
    fn the_spectrum_of_a_constant_is_its_sum_at_bin_zero() {
        let spectrum = spectrum(&[0.5; WAVE_LEN]);
        assert_eq!(spectrum[0], 128.0);
    }
}
