// Ported from SoLoud src/audiosource/speech/resonator.cpp, SoLoud's copy of
// rsynth by Nick Ing-Simmons et al. (public domain; SoLoud's changes public
// domain or CC0, Jari Komppa 2013); altered for SoLouder.

//! Second-order resonators and anti-resonators (`resonator`).

/// SoLoud's `PI`, a `float` literal.
const PI: f32 = std::f32::consts::PI;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Resonator {
    a: f32,
    b: f32,
    c: f32,
    p1: f32,
    p2: f32,
}

impl Resonator {
    /// Coefficients for a frequency and bandwidth in Hz (`initResonator`);
    /// the history is kept.
    pub(super) fn init_resonator(&mut self, frequency: i32, bandwidth: i32, sample_rate: i32) {
        let arg = (-PI / sample_rate as f32) * bandwidth as f32;
        let r = f64::from(arg).exp() as f32;
        self.c = -(r * r);
        let arg = (-2.0 * PI / sample_rate as f32) * frequency as f32;
        self.b = r * (f64::from(arg).cos() as f32) * 2.0;
        self.a = 1.0 - self.b - self.c;
    }

    /// Anti-resonator coefficients (`initAntiresonator`).
    pub(super) fn init_antiresonator(&mut self, frequency: i32, bandwidth: i32, sample_rate: i32) {
        self.init_resonator(frequency, bandwidth, sample_rate);
        self.a = 1.0 / self.a;
        self.b *= -self.a;
        self.c *= -self.a;
    }

    pub(super) fn set_gain(&mut self, gain: f32) {
        self.a *= gain;
    }

    /// Filters a sample, keeping outputs (`resonate`).
    pub(super) fn resonate(&mut self, input: f32) -> f32 {
        let x = self.a * input + self.b * self.p1 + self.c * self.p2;
        self.p2 = self.p1;
        self.p1 = x;
        x
    }

    /// Filters a sample, keeping inputs (`antiresonate`).
    pub(super) fn antiresonate(&mut self, input: f32) -> f32 {
        let x = self.a * input + self.b * self.p1 + self.c * self.p2;
        self.p2 = self.p1;
        self.p1 = input;
        x
    }
}
