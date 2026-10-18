// Ported from SoLoud src/audiosource/sfxr/soloud_sfxr.cpp (zlib/libpng,
// Copyright (c) 2014 Jari Komppa), based on sfxr by Tomas Pettersson,
// relicensed under zlib/libpng by permission; altered for SoLouder.

//! One voice of an sfxr sound (`SfxrInstance`).
//!
//! The arithmetic follows the reference's types: the period and the
//! frequency slides are `double`, everything else `float`; `pow` and `sin`
//! of a `float` run in double precision, as in SoLoud's MSVC build; C's
//! `(int)` conversions go through [`c_int`]. Integer counters that C lets
//! overflow wrap.

use std::f64::consts::PI;

use solouder_core::dsp::c_int;
use solouder_core::{GeneratorContext, GeneratorInstance};

use super::SfxrParams;
use crate::Prg;

/// `pow` of two `float`s, which MSVC's `math.h` computes in `double`.
fn powf(x: f32, y: f32) -> f64 {
    f64::from(x).powf(f64::from(y))
}

#[derive(Debug)]
pub(crate) struct SfxrInstance {
    p: SfxrParams,
    rand: Prg,
    playing: bool,
    phase: i32,
    fperiod: f64,
    fmaxperiod: f64,
    fslide: f64,
    fdslide: f64,
    period: i32,
    square_duty: f32,
    square_slide: f32,
    env_stage: usize,
    env_time: i32,
    env_length: [i32; 3],
    env_vol: f32,
    fphase: f32,
    fdphase: f32,
    iphase: i32,
    phaser: [f32; 1024],
    ipp: i32,
    noise: [f32; 32],
    fltp: f32,
    fltdp: f32,
    fltw: f32,
    fltw_d: f32,
    fltdmp: f32,
    fltphp: f32,
    flthp: f32,
    flthp_d: f32,
    vib_phase: f32,
    vib_speed: f32,
    vib_amp: f32,
    rep_time: i32,
    rep_limit: i32,
    arp_time: i32,
    arp_limit: i32,
    arp_mod: f64,
}

impl SfxrInstance {
    /// A voice of `params`, its noise seeded `0x792352` as in SoLoud.
    pub(crate) fn new(params: SfxrParams) -> Self {
        let mut instance = Self {
            p: params,
            rand: Prg::new(0x0079_2352),
            playing: true,
            phase: 0,
            fperiod: 0.0,
            fmaxperiod: 0.0,
            fslide: 0.0,
            fdslide: 0.0,
            period: 0,
            square_duty: 0.0,
            square_slide: 0.0,
            env_stage: 0,
            env_time: 0,
            env_length: [0; 3],
            env_vol: 0.0,
            fphase: 0.0,
            fdphase: 0.0,
            iphase: 0,
            phaser: [0.0; 1024],
            ipp: 0,
            noise: [0.0; 32],
            fltp: 0.0,
            fltdp: 0.0,
            fltw: 0.0,
            fltw_d: 0.0,
            fltdmp: 0.0,
            fltphp: 0.0,
            flthp: 0.0,
            flthp_d: 0.0,
            vib_phase: 0.0,
            vib_speed: 0.0,
            vib_amp: 0.0,
            rep_time: 0,
            rep_limit: 0,
            arp_time: 0,
            arp_limit: 0,
            arp_mod: 0.0,
        };
        instance.reset(false);
        instance
    }

    /// `frnd(2.0f) - 1.0f`.
    fn noise_sample(&mut self) -> f32 {
        (self.rand.rand() % 10001) as f32 / 10000.0 * 2.0 - 1.0
    }

    /// `resetSample`: the frequency settings, and with `restart` false the
    /// filters, vibrato, envelope, phaser and noise too.
    fn reset(&mut self, restart: bool) {
        let p = self.p;
        if !restart {
            self.phase = 0;
        }
        self.fperiod = 100.0 / (f64::from(p.base_freq * p.base_freq) + 0.001);
        self.period = c_int(self.fperiod);
        self.fmaxperiod = 100.0 / (f64::from(p.freq_limit * p.freq_limit) + 0.001);
        self.fslide = 1.0 - f64::from(p.freq_ramp).powf(3.0) * 0.01;
        self.fdslide = -f64::from(p.freq_dramp).powf(3.0) * 0.000001;
        self.square_duty = 0.5 - p.duty * 0.5;
        self.square_slide = -p.duty_ramp * 0.00005;
        let arp = f64::from(p.arp_mod).powf(2.0);
        self.arp_mod = if p.arp_mod >= 0.0 {
            1.0 - arp * 0.9
        } else {
            1.0 + arp * 10.0
        };
        self.arp_time = 0;
        self.arp_limit = c_int(powf(1.0 - p.arp_speed, 2.0) * 20000.0 + 32.0);
        if p.arp_speed == 1.0 {
            self.arp_limit = 0;
        }
        if restart {
            return;
        }
        self.fltp = 0.0;
        self.fltdp = 0.0;
        self.fltw = powf(p.lpf_freq, 3.0) as f32 * 0.1;
        self.fltw_d = 1.0 + p.lpf_ramp * 0.0001;
        self.fltdmp = 5.0 / (1.0 + powf(p.lpf_resonance, 2.0) as f32 * 20.0) * (0.01 + self.fltw);
        if self.fltdmp > 0.8 {
            self.fltdmp = 0.8;
        }
        self.fltphp = 0.0;
        self.flthp = powf(p.hpf_freq, 2.0) as f32 * 0.1;
        self.flthp_d = (1.0 + f64::from(p.hpf_ramp * 0.0003)) as f32;
        self.vib_phase = 0.0;
        self.vib_speed = powf(p.vib_speed, 2.0) as f32 * 0.01;
        self.vib_amp = p.vib_strength * 0.5;
        self.env_vol = 0.0;
        self.env_stage = 0;
        self.env_time = 0;
        self.env_length =
            [p.env_attack, p.env_sustain, p.env_decay].map(|t| c_int(f64::from(t * t * 100000.0)));
        self.fphase = powf(p.pha_offset, 2.0) as f32 * 1020.0;
        if p.pha_offset < 0.0 {
            self.fphase = -self.fphase;
        }
        self.fdphase = powf(p.pha_ramp, 2.0) as f32 * 1.0;
        if p.pha_ramp < 0.0 {
            self.fdphase = -self.fdphase;
        }
        self.iphase = c_int(f64::from(self.fphase)).wrapping_abs();
        self.ipp = 0;
        self.phaser = [0.0; 1024];
        for k in 0..32 {
            self.noise[k] = self.noise_sample();
        }
        self.rep_time = 0;
        self.rep_limit = c_int(powf(1.0 - p.repeat_speed, 2.0) * 20000.0 + 32.0);
        if p.repeat_speed == 0.0 {
            self.rep_limit = 0;
        }
    }

    /// The sound reached its end: restart when looping, else stop.
    fn finish(&mut self, looping: bool) -> bool {
        if looping {
            self.reset(false);
            true
        } else {
            self.playing = false;
            false
        }
    }

    /// One output sample, or `None` where the sound ends.
    fn sample(&mut self, looping: bool) -> Option<f32> {
        self.rep_time = self.rep_time.wrapping_add(1);
        if self.rep_limit != 0 && self.rep_time >= self.rep_limit {
            self.rep_time = 0;
            self.reset(true);
        }
        self.arp_time = self.arp_time.wrapping_add(1);
        if self.arp_limit != 0 && self.arp_time >= self.arp_limit {
            self.arp_limit = 0;
            self.fperiod *= self.arp_mod;
        }
        self.fslide += self.fdslide;
        self.fperiod *= self.fslide;
        if self.fperiod > self.fmaxperiod {
            self.fperiod = self.fmaxperiod;
            if self.p.freq_limit > 0.0 && !self.finish(looping) {
                return None;
            }
        }
        let mut rfperiod = self.fperiod as f32;
        if self.vib_amp > 0.0 {
            self.vib_phase += self.vib_speed;
            let vibrato = f64::from(self.vib_phase).sin() * f64::from(self.vib_amp);
            rfperiod = (self.fperiod * (1.0 + vibrato)) as f32;
        }
        self.period = c_int(f64::from(rfperiod)).max(8);
        self.square_duty = (self.square_duty + self.square_slide).clamp(0.0, 0.5);
        self.envelope(looping)?;
        self.fphase += self.fdphase;
        self.iphase = c_int(f64::from(self.fphase)).wrapping_abs().min(1023);
        if self.flthp_d != 0.0 {
            // NaN stays NaN, as with the reference's two comparisons.
            self.flthp = (self.flthp * self.flthp_d).clamp(0.00001, 0.1);
        }
        let mut ssample = 0.0_f32;
        for _ in 0..8 {
            ssample += self.supersample() * self.env_vol;
        }
        ssample = ssample / 8.0 * self.p.master_vol;
        ssample *= 2.0 * self.p.sound_vol;
        Some(ssample.clamp(-1.0, 1.0))
    }

    /// Advances the volume envelope; `None` where the sound ends.
    fn envelope(&mut self, looping: bool) -> Option<()> {
        self.env_time = self.env_time.wrapping_add(1);
        if self.env_time > self.env_length[self.env_stage] {
            self.env_time = 0;
            self.env_stage += 1;
            if self.env_stage == 3 && !self.finish(looping) {
                return None;
            }
        }
        let length = self.env_length[self.env_stage];
        let t = self.env_time as f32 / length as f32;
        self.env_vol = match self.env_stage {
            _ if length == 0 => 0.0,
            0 => t,
            1 => 1.0 + powf(1.0 - t, 1.0) as f32 * 2.0 * self.p.env_punch,
            _ => 1.0 - t,
        };
        Some(())
    }

    /// One of the 8 supersamples: waveform, filters and phaser.
    fn supersample(&mut self) -> f32 {
        self.phase = self.phase.wrapping_add(1);
        if self.phase >= self.period {
            self.phase %= self.period;
            if self.p.wave_type == 3 {
                for k in 0..32 {
                    self.noise[k] = self.noise_sample();
                }
            }
        }
        let fp = self.phase as f32 / self.period as f32;
        let mut sample = match self.p.wave_type {
            0 if fp < self.square_duty => 0.5,
            0 => -0.5,
            1 => 1.0 - fp * 2.0,
            2 => (f64::from(fp * 2.0) * PI).sin() as f32,
            // `phase < period`; 64 bits where SoLoud's `int` could overflow.
            3 => self.noise[(i64::from(self.phase) * 32 / i64::from(self.period)) as usize & 31],
            _ => 0.0,
        };
        let pp = self.fltp;
        self.fltw = (self.fltw * self.fltw_d).clamp(0.0, 0.1);
        if self.p.lpf_freq != 1.0 {
            self.fltdp += (sample - self.fltp) * self.fltw;
            self.fltdp -= self.fltdp * self.fltdmp;
        } else {
            self.fltp = sample;
            self.fltdp = 0.0;
        }
        self.fltp += self.fltdp;
        self.fltphp += self.fltp - pp;
        self.fltphp -= self.fltphp * self.flthp;
        sample = self.fltphp;
        self.phaser[(self.ipp & 1023) as usize] = sample;
        let back = self.ipp.wrapping_sub(self.iphase).wrapping_add(1024) & 1023;
        sample += self.phaser[back as usize];
        self.ipp = (self.ipp + 1) & 1023;
        sample
    }
}

impl GeneratorInstance for SfxrInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        _: usize,
        context: &GeneratorContext,
    ) -> usize {
        // An ended sound gives nothing more (reference patch 0074; SoLoud's
        // envelope read past its stages when called again, as the generic
        // seek does).
        if !self.playing {
            return 0;
        }
        for (i, slot) in out[..frames].iter_mut().enumerate() {
            match self.sample(context.looping) {
                Some(sample) => *slot = sample,
                None => return i,
            }
        }
        frames
    }

    fn has_ended(&self) -> bool {
        !self.playing
    }
}
