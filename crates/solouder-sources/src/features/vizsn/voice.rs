// Ported from SoLoud src/audiosource/vizsn/soloud_vizsn.cpp: vizsn by
// Ville-Matias Heikkilä, integrated into SoLoud by Jari Komppa (2013-2018),
// both released under the WTFPL (http://www.wtfpl.net/txt/copying/); altered
// for SoLouder.

//! The phones and the synthesiser: resonator banks, the voicing and noise
//! sources, and the echo that ends each sample (`genwave`).

use solouder_core::dsp::c_int;

/// The phone that clears every resonator, and the end of a text.
pub(super) const P_CLR: i8 = -2;
pub(super) const P_END: i8 = -1;
const P_K: i8 = 14;
const P_P: i8 = 16;

/// Phones of `a` to `|` (`keyz`); `{` and `|` are ä and ö.
pub(super) const KEYZ: [i8; 28] = [
    0, 16, 11, 15, 2, 9, 19, 8, 5, 10, 14, 12, 18, 17, 4, 16, 14, 13, 11, 15, 7, 9, 7, 11, 6, 11,
    1, 3,
];

/// Plosives (k, t, p) and the clearing phone (`plosive`).
pub(super) fn plosive(phone: i8) -> bool {
    (P_K..=P_P).contains(&phone) || phone == P_CLR
}

// Resonator indices.
const RLP: usize = 0;
const RNZ: usize = 1;
const RNPC: usize = 2;
const ROUT: usize = 3;
const R1P: usize = 4;
const R2P: usize = 5;
const R3P: usize = 6;
const R4P: usize = 7;

/// Each vowel's `a` and `b` for the four formant resonators (`vowtab`).
const VOWELS: [[f32; 8]; 8] = [
    [0.10, 1.6, 0.0, 0.0, 0.2, 1.5, 0.0, 0.0],  // a
    [0.10, 1.6, 0.0, 0.0, 0.2, 0.0, 0.0, 0.0],  // ä
    [0.08, 1.8, 0.0, 0.0, 0.2, -0.8, 0.0, 0.0], // e
    [0.08, 1.8, 0.0, 0.0, 0.3, 0.9, 0.0, 0.0],  // ö
    [0.08, 1.8, 0.0, 0.0, 0.2, 1.6, 0.0, 0.0],  // o
    [0.05, 1.9, 0.0, 0.0, 0.2, -1.5, 0.0, 0.0], // i
    [0.05, 1.9, 0.0, 0.0, 0.2, 0.8, 0.0, 0.0],  // y
    [0.05, 1.9, 0.0, 0.0, 0.1, 1.7, 0.0, 0.0],  // u
];

/// Consonants (`voo`): voice (unused), aspiration, frication, bypass,
/// breath (unused), then `a`, `b` and `c` of each resonator.
#[rustfmt::skip]
const CONSONANTS: [[f32; 35]; 12] = [
    // h
    [0.0, 0.0, 0.36, 0.0, 0.0, 0.32, 1.36, -0.67, 22.0, -42.0, 21.0, 0.04, 1.9, -0.92, 1.17, 0.8, -0.02,
     0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // v
    [0.7, 0.0, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 22.0, -42.0, 21.0, 0.04, 1.9, -0.92, 1.17, 0.8, -0.02,
     0.005, 1.9, -0.95, 0.0, 0.0, 0.0, 0.04, 1.5, -0.93, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // j
    [0.7, 0.0, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 22.0, -42.0, 21.0, 0.04, 1.9, -0.92, 1.17, 0.8, -0.02,
     0.005, 1.9, -0.95, 0.0, 0.0, 0.0, 0.04, -1.5, -0.93, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // s
    [0.0, 0.0, 0.36, 0.0, 0.0, 0.32, 1.36, -0.67, 22.0, -42.0, 21.0, 0.04, 1.9, -0.92, 1.7, 0.1, -0.02,
     0.0, 0.1, -0.02, 0.0, 1.76, -0.85, 0.01, 0.42, -0.93, 0.02, -1.37, -0.68, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // l
    [0.8, 0.0, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 22.0, -42.0, 21.0, 0.04, 1.88, -0.92, 1.17, 0.8, -0.02,
     0.1, -0.5, -0.93, 0.6, 1.0, -0.93, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // r
    [0.8, 0.4, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 22.0, -42.0, 21.0, 0.04, 1.9, -0.92, 1.17, 0.8, -0.02,
     -0.2, 0.0, -0.93, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // k
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 22.0, -42.0, 21.0, 0.04, 1.9, -0.92, 1.17, 0.8, -0.02,
     0.0, 1.95, -0.94, 0.4, 1.0, -0.93, 0.6, 1.0, -0.89, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // t
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 22.0, -42.0, 21.0, 0.04, 1.9, -0.92, 1.17, 0.8, -0.02,
     0.0, 1.6, -0.94, 0.0, 0.3, -0.93, 1.5, -0.6, -0.89, 1.8, -1.5, -0.68, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // p
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 22.0, -42.0, 21.0, 0.04, 1.9, -0.92, 1.17, 0.8, -0.02,
     0.01, 1.9, -0.94, 1.5, 1.7, -0.93, 1.0, 1.0, -0.89, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // n
    [0.81, 0.0, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 15.0, -20.0, 10.0, 0.04, 1.88, -0.92, 1.17, 0.1, -0.02,
     0.02, 1.83, -0.97, 0.10, 0.26, -0.83, 0.06, -0.85, -0.82, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // m
    [0.81, 0.0, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 15.0, -20.0, 10.0, 0.04, 1.88, -0.92, 1.17, 0.1, -0.02,
     0.08, 1.5, -0.94, 0.06, 1.0, -0.9, 0.05, 1.5, -0.89, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    // ng
    [0.81, 0.0, 0.0, 0.0, 0.0, 0.32, 1.36, -0.67, 15.0, -20.0, 10.0, 0.04, 1.88, -0.92, 1.17, 0.1, -0.02,
     0.1, 1.6, -0.94, 0.7, -1.0, -0.91, 0.1, 1.16, -0.91, 0.03, -1.3, -0.68, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
];

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Resonator {
    a: f32,
    b: f32,
    c: f32,
    p1: f32,
    p2: f32,
}

impl Resonator {
    fn resonate(&mut self, i: f32) -> f32 {
        let x = (self.a * i) + (self.b * self.p1) + (self.c * self.p2);
        self.p2 = self.p1;
        self.p1 = x;
        x
    }

    fn antiresonate(&mut self, i: f32) -> f32 {
        let x = self.a * i + self.b * self.p1 + self.c * self.p2;
        self.p2 = self.p1;
        self.p1 = i;
        x
    }
}

/// A phone's resonators and amplitudes (`VizsnBank`).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Bank {
    r: [Resonator; 10],
    pitch: f32,
    frica: f32,
    voice: f32,
    aspir: f32,
    bypas: f32,
    breth: f32,
}

impl Bank {
    /// The bank of `phone` (`setphone`); resonator histories are kept, except
    /// that a negative phone clears everything.
    pub(super) fn set_phone(&mut self, phone: i8, pitch: f32) {
        self.frica = 0.0;
        self.aspir = 0.0;
        self.bypas = 0.0;
        self.breth = 0.0;
        self.voice = 0.0;
        self.pitch = pitch;
        if phone < 0 {
            self.r = [Resonator::default(); 10];
        } else if phone < 8 {
            let r = &mut self.r;
            r[R1P].c = -0.95;
            r[R2P].c = -0.93;
            r[R3P].c = -0.88;
            r[R4P].c = -0.67;
            (r[RLP].a, r[RLP].b, r[RLP].c) = (0.31, 1.35, -0.67);
            (r[RNZ].a, r[RNZ].b, r[RNZ].c) = (22.0, -42.0, 21.0);
            (r[RNPC].a, r[RNPC].b, r[RNPC].c) = (0.04, 1.87, -0.92);
            (r[ROUT].a, r[ROUT].b, r[ROUT].c) = (1.16, 0.08, -0.02);
            let vowel = &VOWELS[phone as usize];
            for k in 0..4 {
                r[R1P + k].a = vowel[2 * k];
                r[R1P + k].b = vowel[2 * k + 1];
            }
            self.voice = 0.8;
        } else {
            let v = &CONSONANTS[phone as usize - 8];
            self.aspir = v[1];
            self.frica = v[2];
            self.bypas = v[3];
            for (j, r) in self.r.iter_mut().enumerate() {
                (r.a, r.b, r.c) = (v[5 + 3 * j], v[6 + 3 * j], v[7 + 3 * j]);
            }
            self.voice = 0.8;
            self.breth = 0.18;
        }
    }

    /// The per-tick step from `self` to `to` over `ticks` (`slidePrepare`).
    pub(super) fn slide(&self, to: &Bank, ticks: f32) -> Bank {
        let mut step = Bank::default();
        for (s, (a, b)) in step.r.iter_mut().zip(self.r.iter().zip(&to.r)) {
            s.a = (b.a - a.a) / ticks;
            s.b = (b.b - a.b) / ticks;
            s.c = (b.c - a.c) / ticks;
        }
        step.pitch = (to.pitch - self.pitch) / ticks;
        step
    }

    /// One step of a slide (`slideTick`).
    pub(super) fn tick(&mut self, step: &Bank) {
        for (r, s) in self.r.iter_mut().zip(&step.r) {
            r.a += s.a;
            r.b += s.b;
            r.c += s.c;
        }
        self.pitch += step.pitch;
    }
}

/// The synthesiser's running state.
#[derive(Clone, Debug)]
pub(super) struct Voice {
    pub(super) bank: Bank,
    pub(super) nper: i32,
    echo: [i32; 1024],
    ptr: usize,
    /// The sawtooth's phase (`mA`), wrapping as MSVC's `int` does.
    a: i32,
    /// The voice type the phase was reset for (`mOrgv`).
    orgv: i32,
    glot_last: f32,
    rand_state: u32,
}

/// SoLoud's only voice type (`mCurrentVoiceType`): a sawtooth.
const VOICE_TYPE: i32 = 6;

impl Voice {
    pub(super) fn new() -> Self {
        Self {
            bank: Bank::default(),
            nper: 0,
            echo: [0; 1024],
            ptr: 0,
            a: 0,
            orgv: -1,
            glot_last: 0.0,
            rand_state: 1,
        }
    }

    /// MSVC's `rand()`, per voice (reference patch 0105).
    fn rand(&mut self) -> i32 {
        self.rand_state = self
            .rand_state
            .wrapping_mul(214_013)
            .wrapping_add(2_531_011);
        ((self.rand_state >> 16) & 0x7fff) as i32
    }

    /// The voicing source (`vcsrc`); SoLoud never chooses a voice type but
    /// the sawtooth, so the others are not kept.
    fn voicing(&mut self, pitch: i32) -> f32 {
        self.a = self.a.wrapping_add(pitch);
        if self.orgv != VOICE_TYPE {
            self.orgv = VOICE_TYPE;
            self.a = 0;
        }
        (self.a & 65535) as f32 * (0.001 / 256.0)
    }

    /// One sample (`genwave`); `nmod` and `nopen` are always 0 in SoLoud.
    pub(super) fn sample(&mut self) -> f32 {
        let mut noise = (self.rand() & 32767) as f32 / 32768.0;
        if self.nper > 0 {
            noise *= 0.5;
        }
        let b = &mut self.bank;
        let mut s = b.frica * noise;
        let pitch = c_int(f64::from(b.pitch).floor());
        let voice = self.voicing(pitch);
        let b = &mut self.bank;
        let voice = b.r[RLP].resonate(voice);
        let mut parglot = (b.voice * voice) + (b.aspir * noise);
        parglot = b.r[RNZ].antiresonate(parglot);
        parglot = b.r[RNPC].resonate(parglot);
        s += parglot - self.glot_last;
        self.glot_last = parglot;
        let mut o = b.r[R1P].resonate(parglot);
        for i in [R4P, R3P] {
            o = b.r[i].resonate(s) - o;
        }
        o = b.r[ROUT].resonate(b.bypas * s - o);
        let echo = (self.echo[self.ptr] / 4) as f32;
        let ob = c_int(f64::from(o * 400.0 * 256.0 + echo).floor());
        self.echo[self.ptr] = ob;
        self.ptr = (self.ptr + 1) & 1023;
        let ob = ((ob >> 8) + 128).clamp(0, 255);
        self.nper += 1;
        ob as f32 * (1.0 / 255.0)
    }
}
