// Ported from SoLoud src/audiosource/speech/klatt.cpp, SoLoud's copy of
// rsynth by Nick Ing-Simmons et al. (public domain; SoLoud's changes public
// domain or CC0, Jari Komppa 2013); altered for SoLouder.

//! The Klatt formant synthesiser's sample loop (`klatt::parwave` and what
//! it calls). SoLoud never turns on F0 flutter (`mF0Flutter` is always 0),
//! and never reads the cascade bandwidths, the glottal low-pass or the
//! parallel nasal pole it sets; they are not kept. `(int)` conversions go
//! through [`c_int`].

use solouder_core::dsp::c_int;

use super::KlattWaveform;
use super::resonator::Resonator;

/// The synthesiser's rate (`mSampleRate`), fixed by `klatt::init`.
pub(super) const SAMPLE_RATE: i32 = 11025;

/// One frame's parameters (`klatt_frame`); only the fields klatt reads.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub(super) f0: i32,
    pub(super) voicing_db: i32,
    pub(super) formant_freq: [i32; 6],
    pub(super) parallel_bandwidth: [i32; 6],
    pub(super) formant_db: [i32; 6],
    pub(super) nasal_zero_freq: i32,
    nasal_zero_bandwidth: i32,
    pub(super) nasal_pole_freq: i32,
    nasal_pole_bandwidth: i32,
    pub(super) aspiration_db: i32,
    open_samples: i32,
    pub(super) breathiness: i32,
    tilt_db: i32,
    pub(super) frication_db: i32,
    skewness: i32,
    pub(super) bypass_db: i32,
    gain_db: i32,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            f0: 1330,
            voicing_db: 60,
            formant_freq: [500, 1500, 2800, 3250, 3700, 4990],
            parallel_bandwidth: [80, 200, 350, 500, 600, 800],
            formant_db: [0; 6],
            nasal_zero_freq: 270,
            nasal_zero_bandwidth: 100,
            nasal_pole_freq: 270,
            nasal_pole_bandwidth: 100,
            aspiration_db: 0,
            open_samples: 30,
            breathiness: 0,
            tilt_db: 10,
            frication_db: 0,
            skewness: 0,
            bypass_db: 0,
            gain_db: 62,
        }
    }
}

/// Decibels to a linear gain, 87 dB giving 32.767 (`DBtoLIN`).
fn db_to_lin(db: i32) -> f32 {
    const AMPLITUDES: [f32; 88] = [
        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 6.0, 7.0, 8.0, 9.0, 10.0,
        11.0, 13.0, 14.0, 16.0, 18.0, 20.0, 22.0, 25.0, 28.0, 32.0, 35.0, 40.0, 45.0, 51.0, 57.0,
        64.0, 71.0, 80.0, 90.0, 101.0, 114.0, 128.0, 142.0, 159.0, 179.0, 202.0, 227.0, 256.0,
        284.0, 318.0, 359.0, 405.0, 455.0, 512.0, 568.0, 638.0, 719.0, 811.0, 911.0, 1024.0,
        1137.0, 1276.0, 1438.0, 1622.0, 1823.0, 2048.0, 2273.0, 2552.0, 2875.0, 3244.0, 3645.0,
        4096.0, 4547.0, 5104.0, 5751.0, 6488.0, 7291.0, 8192.0, 9093.0, 10207.0, 11502.0, 12976.0,
        14582.0, 16384.0, 18350.0, 20644.0, 23429.0, 26214.0, 29491.0, 32767.0,
    ];
    AMPLITUDES[db.clamp(0, 87) as usize] * 0.001
}

/// A sample clipped to 16 bits (`clip`).
fn clip(input: f32) -> i16 {
    c_int(f64::from(input)).clamp(-32767, 32767) as i16
}

/// The synthesiser (`klatt`).
#[derive(Clone, Debug, Default)]
pub(super) struct Klatt {
    pub(super) base_f0: i32,
    pub(super) base_declination: f32,
    waveform: KlattWaveform,
    /// Samples per frame (`mNspFr`).
    pub(super) frame_samples: i32,
    f0: i32,
    voicing_db: i32,
    skewness: i32,
    nper: i32,
    t0: i32,
    nopen: i32,
    nmod: i32,
    amp_voice: f32,
    amp_bypass: f32,
    amp_aspiration: f32,
    amp_frication: f32,
    amp_breath: f32,
    skew: i32,
    vlast: f32,
    nlast: f32,
    glot_last: f32,
    decay: f32,
    one_md: f32,
    pub(super) seed: u32,
    pub(super) frame: Frame,
    parallel: [Resonator; 6],
    nasal_pole: Resonator,
    nasal_zero: Resonator,
    down_sample_low_pass: Resonator,
    output_low_pass: Resonator,
    pub(super) synth: super::synth::SynthState,
}

impl Klatt {
    /// A synthesiser as `klatt::klatt` leaves it.
    pub(super) fn new() -> Self {
        Self {
            base_f0: 1330,
            base_declination: 0.5,
            seed: 5,
            ..Self::default()
        }
    }

    /// `klatt::init`: the base parameters, the frame size, and the voicing
    /// source's state. SoLoud's voices call it at every read, which resets
    /// the glottal period and the low-pass histories' inputs (kept).
    pub(super) fn init(&mut self, f0: i32, speed: f32, declination: f32, waveform: KlattWaveform) {
        self.base_f0 = f0;
        self.base_declination = declination;
        self.waveform = waveform;
        self.f0 = f0;
        self.frame.f0 = f0;
        let flp_hz = (950 * SAMPLE_RATE) / 10000;
        let blp_hz = (630 * SAMPLE_RATE) / 10000;
        self.frame_samples = c_int(f64::from(SAMPLE_RATE as f32 * speed)) / 1000;
        self.down_sample_low_pass
            .init_resonator(flp_hz, blp_hz, SAMPLE_RATE);
        self.nper = 0;
        self.t0 = 0;
        self.vlast = 0.0;
        self.nlast = 0.0;
        self.glot_last = 0.0;
    }

    /// The voicing waveform during the open phase (`natural_source`).
    fn natural_source(&self, nper: i32) -> f32 {
        if nper >= self.nopen {
            return 0.0; // glottis closed
        }
        let phase = (nper % 200) - 100;
        match self.waveform {
            KlattWaveform::Triangle => phase as f32 * 81.92,
            KlattWaveform::Sin => ((f64::from(nper) * 0.0314).sin() * 8192.0) as f32,
            KlattWaveform::Square => {
                if phase > 0 {
                    8192.0
                } else {
                    -8192.0
                }
            }
            KlattWaveform::Pulse => {
                if phase > 50 {
                    8192.0
                } else {
                    -8192.0
                }
            }
            KlattWaveform::Noise => {
                if c_int(f64::from(self.nlast)) & 1 != 0 {
                    -8192.0
                } else {
                    8192.0
                }
            }
            KlattWaveform::Warble => {
                if c_int(f64::from(self.nlast)) & 7 != 0 {
                    -8192.0
                } else {
                    8192.0
                }
            }
            KlattWaveform::Saw => (phase.abs() - 50) as f32 * 163.84,
        }
    }

    /// Parameters reset at each glottal period (`pitch_synch_par_reset`).
    fn pitch_synch_par_reset(&mut self, ns: i32) {
        if self.f0 > 0 {
            self.t0 = (40 * SAMPLE_RATE) / self.f0;
            self.amp_voice = db_to_lin(self.voicing_db);
            self.nmod = self.t0;
            if self.voicing_db > 0 {
                self.nmod >>= 1;
            }
            self.amp_breath = db_to_lin(self.frame.breathiness) * 0.1;
            self.nopen = 4 * self.frame.open_samples;
            if self.nopen >= self.t0 - 1 {
                self.nopen = self.t0 - 2;
            }
            if self.nopen < 40 {
                self.nopen = 40;
            }
            let closed = self.t0 - self.nopen;
            if self.skewness > closed {
                self.skewness = closed;
            }
            self.skew = if self.skew >= 0 {
                self.skewness
            } else {
                -self.skewness
            };
            self.t0 += self.skew;
            self.skew = -self.skew;
        } else {
            self.t0 = 4;
            self.amp_voice = 0.0;
            self.nmod = self.t0;
            self.amp_breath = 0.0;
        }
        if self.t0 != 4 || ns == 0 {
            self.decay = 0.033 * self.frame.tilt_db as f32;
            self.one_md = if self.decay > 0.0 {
                1.0 - self.decay
            } else {
                1.0
            };
        }
    }

    /// The frame's parameters into the filters (`frame_init`).
    fn frame_init(&mut self) {
        const PARALLEL_GAIN: [f32; 6] = [0.4, 0.15, 0.06, 0.04, 0.022, 0.03];
        let frame = self.frame;
        self.f0 = frame.f0;
        self.voicing_db = frame.voicing_db.wrapping_sub(7).max(0);
        self.amp_aspiration = db_to_lin(frame.aspiration_db) * 0.05;
        self.amp_frication = db_to_lin(frame.frication_db) * 0.25;
        self.skewness = frame.skewness;
        self.amp_bypass = db_to_lin(frame.bypass_db) * 0.05;
        self.nasal_pole.init_resonator(
            frame.nasal_pole_freq,
            frame.nasal_pole_bandwidth,
            SAMPLE_RATE,
        );
        self.nasal_zero.init_antiresonator(
            frame.nasal_zero_freq,
            frame.nasal_zero_bandwidth,
            SAMPLE_RATE,
        );
        for (i, resonator) in self.parallel.iter_mut().enumerate() {
            let gain = db_to_lin(frame.formant_db[i]) * PARALLEL_GAIN[i];
            resonator.init_resonator(
                frame.formant_freq[i],
                frame.parallel_bandwidth[i],
                SAMPLE_RATE,
            );
            resonator.set_gain(gain);
        }
        let mut gain_db = frame.gain_db - 3;
        if gain_db <= 0 {
            gain_db = 57;
        }
        self.output_low_pass
            .init_resonator(0, SAMPLE_RATE, SAMPLE_RATE);
        self.output_low_pass.set_gain(db_to_lin(gain_db));
    }

    /// One frame of samples into `out`, which holds `frame_samples`
    /// (`parwave`).
    pub(super) fn parwave(&mut self, out: &mut [i16]) {
        self.frame_init();
        for (ns, sample) in out.iter_mut().enumerate() {
            // SoLoud's own LCG: the top 14 bits of the seed.
            self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1);
            let nrand = (self.seed as i32) >> 18;
            let mut noise = nrand as f32 + 0.75 * self.nlast;
            self.nlast = noise;
            if self.nper > self.nmod {
                noise *= 0.5;
            }
            let mut source = self.amp_frication * noise;
            // The glottal source runs at 4 times the rate.
            let mut voice = 0.0;
            for _ in 0..4 {
                voice = self.natural_source(self.nper);
                if self.nper >= self.t0 {
                    self.nper = 0;
                    self.pitch_synch_par_reset(ns as i32);
                }
                voice = self.down_sample_low_pass.resonate(voice);
                self.nper += 1;
            }
            voice = (voice * self.one_md) + (self.vlast * self.decay);
            self.vlast = voice;
            if self.nper < self.nopen {
                voice += self.amp_breath * nrand as f32;
            }
            let mut glot_out = self.amp_voice * voice;
            glot_out += self.amp_aspiration * noise;
            // The glottal source through the nasal resonators (rsynth).
            let mut par_glot_out = self.nasal_zero.antiresonate(glot_out);
            par_glot_out = self.nasal_pole.resonate(par_glot_out);
            let mut out = self.parallel[0].resonate(par_glot_out);
            source += par_glot_out - self.glot_last;
            self.glot_last = par_glot_out;
            for resonator in self.parallel[1..].iter_mut().rev() {
                out = resonator.resonate(source) - out;
            }
            out = self.amp_bypass * source - out;
            out = self.output_low_pass.resonate(out);
            *sample = clip(out);
        }
    }
}
