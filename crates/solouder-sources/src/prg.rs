// Ported from SoLoud src/core/soloud_misc.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder. The generator
// is WELL512, public domain by Chris Lomont.

//! SoLoud's pseudorandom numbers (`Misc::Prg`): WELL512.

/// SoLoud's `Misc::Prg`, a WELL512 generator. Deterministic: two generators
/// seeded alike produce the same numbers on every platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prg {
    state: [u32; 16],
    index: usize,
}

impl Default for Prg {
    /// Seeded with 0, as SoLoud's constructor does.
    fn default() -> Self {
        Self::new(0)
    }
}

impl Prg {
    /// A generator seeded with `seed` (`Prg::srand`).
    pub fn new(seed: i32) -> Self {
        let mut prg = Self {
            state: [0; 16],
            index: 0,
        };
        prg.srand(seed);
        prg
    }

    /// Restarts the sequence from `seed` (`Prg::srand`). The state words
    /// are `seed + i * seed + i`, in C's wrapping `int` arithmetic.
    pub fn srand(&mut self, seed: i32) {
        self.index = 0;
        for (i, word) in self.state.iter_mut().enumerate() {
            let i = i as i32;
            *word = seed.wrapping_add(i.wrapping_mul(seed)).wrapping_add(i) as u32;
        }
    }

    /// The next number (`Prg::rand`).
    pub fn rand(&mut self) -> u32 {
        let s = &mut self.state;
        let mut a = s[self.index];
        let mut c = s[(self.index + 13) & 15];
        let b = a ^ c ^ (a << 16) ^ (c << 15);
        c = s[(self.index + 9) & 15];
        c ^= c >> 11;
        s[self.index] = b ^ c;
        a = s[self.index];
        let d = a ^ ((a << 5) & 0xDA44_2D24);
        self.index = (self.index + 15) & 15;
        a = s[self.index];
        s[self.index] = a ^ b ^ d ^ (a << 2) ^ (b << 18) ^ (c << 28);
        s[self.index]
    }

    /// The next number scaled to `[0, 1]` (`Prg::rand_float`): rounded to
    /// `f32`, then times 2^-32.
    pub fn rand_float(&mut self) -> f32 {
        self.rand() as f32 * 2.328_306_4e-10_f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_fill_the_state_with_wrapping_arithmetic() {
        let prg = Prg::new(0x4000_0000);
        assert_eq!(prg.state[0], 0x4000_0000);
        // 0x40000000 + 3 * 0x40000000 + 3 wraps to 3.
        assert_eq!(prg.state[3], 3);
    }

    #[test]
    fn the_sequence_is_deterministic() {
        let mut a = Prg::new(0xfade);
        let mut b = Prg::new(0xfade);
        let first: Vec<u32> = (0..40).map(|_| a.rand()).collect();
        assert!(
            first
                .iter()
                .zip((0..40).map(|_| b.rand()))
                .all(|(x, y)| *x == y)
        );
        assert_ne!(first[0], first[1]);
        let mut c = Prg::new(1);
        assert!((0..1000).all(|_| (0.0..=1.0).contains(&c.rand_float())));
    }
}
