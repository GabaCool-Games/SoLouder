// Ported from SoLoud src/filter/soloud_fftfilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder. Its
// transformations are largely based on smbPitchShift.cpp, COPYRIGHT
// 1999-2015 Stephan M. Bernsee, under The Wide Open License (WOL):
// "Permission to use, copy, modify, distribute and sell this software and its
// documentation for any purpose is hereby granted without fee, provided that
// the above copyright notice and this license appear in all source copies.
// THIS SOFTWARE IS PROVIDED "AS IS" WITHOUT EXPRESS OR IMPLIED WARRANTY OF
// ANY KIND. See http://www.dspguru.com/wol.htm for more information."

use std::f32::consts::PI as PI_F32;
use std::f64::consts::PI;

use super::{Filter, FilterInstance, FilterParams};
use crate::contracts::MAX_CHANNELS;
use crate::dsp::{c_int, fft};

pub(super) const WINDOW: usize = 256;
const HALF: u32 = 128;
const TWICE: usize = 512;
/// Complex bins handed to a spectrum transform.
pub(super) const BINS: usize = 128;

/// A short-time Fourier transform pitch shift that drops the spectrum an
/// octave (`FFTFilter`); the base of [`super::BassboostFilter`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FftFilter;

impl FftFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;
}

impl Filter for FftFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        Box::new(FftInstance {
            params: FilterParams::new(1),
            stft: Stft::default(),
        })
    }
}

#[derive(Debug)]
struct FftInstance {
    params: FilterParams,
    stft: Stft,
}

impl FilterInstance for FftInstance {
    fn params(&self) -> &FilterParams {
        &self.params
    }

    fn params_mut(&mut self) -> &mut FilterParams {
        &mut self.params
    }

    fn filter_channel(
        &mut self,
        samples: &mut [f32],
        sample_rate: f32,
        time: f64,
        channel: usize,
        channels: usize,
    ) {
        if channel == 0 {
            self.params.update(time);
        }
        let wet = self.params.values()[0];
        self.stft
            .filter_channel(samples, wet, channel, channels, |spectrum, phases| {
                shift_down(spectrum, phases, sample_rate);
            });
    }
}

fn shift_down(spectrum: &mut [f32; WINDOW], phases: Phases<'_>, rate: f32) {
    comp_to_mag_phase(spectrum, BINS);
    mag_phase_to_mag_freq(spectrum, phases.last, rate);
    let mut t = [0.0; BINS];
    t.copy_from_slice(&spectrum[..BINS]);
    spectrum.fill(0.0);
    for i in 0..BINS / 4 {
        let d = i * 2;
        if d < BINS / 4 {
            spectrum[d * 2] += t[i * 2];
            spectrum[d * 2 + 1] = t[i * 2 + 1] * 2.0;
        }
    }
    mag_freq_to_mag_phase(spectrum, phases.sum, rate);
    mag_phase_to_comp(spectrum, BINS);
}

/// One channel's phase history for the magnitude/frequency transforms.
pub(super) struct Phases<'a> {
    last: &'a mut [f32],
    sum: &'a mut [f32],
}

/// The overlap-add frame (`FFTFilterInstance` buffers and offsets): windows
/// of 256 samples every 128, each transformed, handed to a spectrum
/// transform, transformed back and mixed in with a triangular window.
#[derive(Debug)]
pub(super) struct Stft {
    input: Vec<f32>,
    mix: Vec<f32>,
    last_phase: Vec<f32>,
    sum_phase: Vec<f32>,
    input_offset: [u32; MAX_CHANNELS as usize],
    mix_offset: [u32; MAX_CHANNELS as usize],
    read_offset: [u32; MAX_CHANNELS as usize],
}

impl Default for Stft {
    fn default() -> Self {
        Self {
            input: Vec::new(),
            mix: Vec::new(),
            last_phase: Vec::new(),
            sum_phase: Vec::new(),
            input_offset: [WINDOW as u32; MAX_CHANNELS as usize],
            mix_offset: [HALF; MAX_CHANNELS as usize],
            read_offset: [0; MAX_CHANNELS as usize],
        }
    }
}

impl Stft {
    pub(super) fn filter_channel(
        &mut self,
        samples: &mut [f32],
        wet: f32,
        channel: usize,
        channels: usize,
        mut transform: impl FnMut(&mut [f32; WINDOW], Phases<'_>),
    ) {
        if self.input.len() < TWICE * channels {
            self.input.resize(TWICE * channels, 0.0);
            self.mix.resize(TWICE * channels, 0.0);
            self.last_phase.resize(WINDOW * channels, 0.0);
            self.sum_phase.resize(WINDOW * channels, 0.0);
        }
        let ring = |offset: u32| offset as usize & (TWICE - 1);
        let input = &mut self.input[TWICE * channel..][..TWICE];
        let mix = &mut self.mix[TWICE * channel..][..TWICE];
        let mut input_ofs = self.input_offset[channel];
        let mut mix_ofs = self.mix_offset[channel];
        let mut read_ofs = self.read_offset[channel];
        let len = samples.len();
        let mut ofs = 0;
        while ofs < len {
            let count = ((HALF - (input_ofs & (HALF - 1))) as usize).min(len - ofs);
            for &sample in &samples[ofs..ofs + count] {
                let at = ring(input_ofs.wrapping_add(HALF));
                input[at] = sample;
                mix[at] = 0.0;
                input_ofs = input_ofs.wrapping_add(1);
            }
            if input_ofs & (HALF - 1) == 0 {
                let mut temp = [0.0; WINDOW];
                let start = input_ofs.wrapping_add(TWICE as u32 - HALF);
                for (i, value) in temp.iter_mut().enumerate() {
                    *value = input[ring(start.wrapping_add(i as u32))];
                }
                fft::fft(&mut temp);
                let phases = Phases {
                    last: &mut self.last_phase[WINDOW * channel..][..WINDOW],
                    sum: &mut self.sum_phase[WINDOW * channel..][..WINDOW],
                };
                transform(&mut temp, phases);
                fft::ifft(&mut temp);
                for (i, &value) in temp.iter().enumerate() {
                    let weight = HALF as f32 - (HALF as i32 - i as i32).abs() as f32;
                    mix[ring(mix_ofs)] += value * weight * (1.0 / HALF as f32);
                    mix_ofs = mix_ofs.wrapping_add(1);
                }
                mix_ofs = mix_ofs.wrapping_sub(HALF);
            }
            for sample in &mut samples[ofs..ofs + count] {
                *sample += (mix[ring(read_ofs)] - *sample) * wet;
                read_ofs = read_ofs.wrapping_add(1);
            }
            ofs += count;
        }
        self.input_offset[channel] = input_ofs;
        self.read_offset[channel] = read_ofs;
        self.mix_offset[channel] = mix_ofs;
    }
}

/// The first `bins` complex values to magnitude (doubled) and phase
/// (`comp2MagPhase`).
pub(super) fn comp_to_mag_phase(spectrum: &mut [f32], bins: usize) {
    for pair in &mut spectrum.as_chunks_mut::<2>().0[..bins] {
        let [re, im] = *pair;
        *pair = [
            f64::from(re * re + im * im).sqrt() as f32 * 2.0,
            f64::from(im).atan2(f64::from(re)) as f32,
        ];
    }
}

/// The inverse of [`comp_to_mag_phase`] (`magPhase2Comp`).
pub(super) fn mag_phase_to_comp(spectrum: &mut [f32], bins: usize) {
    for pair in &mut spectrum.as_chunks_mut::<2>().0[..bins] {
        let [mag, phase] = *pair;
        let phase = f64::from(phase);
        *pair = [phase.cos() as f32 * mag, phase.sin() as f32 * mag];
    }
}

/// Phases to true bin frequencies from the change since the previous
/// window (`magPhase2MagFreq`).
fn mag_phase_to_mag_freq(spectrum: &mut [f32; WINDOW], last: &mut [f32], rate: f32) {
    let size = BINS as f32;
    let step = size / rate;
    let expected = (step / size) * 2.0 * PI_F32;
    let per_bin = rate / size;
    for (i, pair) in spectrum.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let phase = pair[1];
        let mut freq = phase - last[i];
        last[i] = phase;
        freq -= i as f32 * expected;
        let mut qpd = c_int((f64::from(freq) / PI).floor());
        if qpd >= 0 {
            qpd = qpd.wrapping_add(qpd & 1);
        } else {
            qpd = qpd.wrapping_sub(qpd & 1);
        }
        freq -= PI_F32 * qpd as f32;
        freq = size * freq / (2.0 * PI_F32);
        pair[1] = i as f32 * per_bin + freq * per_bin;
    }
}

/// True bin frequencies back to accumulated phases (`magFreq2MagPhase`).
fn mag_freq_to_mag_phase(spectrum: &mut [f32; WINDOW], sum: &mut [f32], rate: f32) {
    let size = BINS as f32;
    let step = size / rate;
    let expected = (step / size) * 2.0 * PI_F32;
    let per_bin = rate / size;
    for (i, pair) in spectrum.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let mut freq = pair[1];
        freq -= i as f32 * per_bin;
        freq /= per_bin;
        freq = (freq / size) * PI_F32 * 2.0;
        freq += i as f32 * expected;
        sum[i] += freq;
        pair[1] = sum[i];
    }
}
