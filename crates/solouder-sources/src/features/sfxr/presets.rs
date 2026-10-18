// Ported from SoLoud src/audiosource/sfxr/soloud_sfxr.cpp (zlib/libpng,
// Copyright (c) 2014 Jari Komppa), based on sfxr by Tomas Pettersson,
// relicensed under zlib/libpng by permission; altered for SoLouder.

//! Sfxr's random presets (`Sfxr::loadPreset`).

use super::SfxrParams;
use crate::Prg;

/// One of sfxr's seven sound kinds, randomized by a seed
/// ([`Sfxr::load_preset`](super::Sfxr::load_preset)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SfxrPreset {
    /// Pickup or coin.
    Coin,
    /// Laser or shot.
    Laser,
    /// Explosion.
    Explosion,
    /// Power-up.
    Powerup,
    /// Hit or hurt.
    Hurt,
    /// Jump.
    Jump,
    /// Blip or menu select.
    Blip,
}

impl SfxrPreset {
    /// The preset of SoLoud's number (`SFXR_PRESETS`, 0 to 6).
    pub fn from_index(index: u32) -> Option<Self> {
        Some(match index {
            0 => Self::Coin,
            1 => Self::Laser,
            2 => Self::Explosion,
            3 => Self::Powerup,
            4 => Self::Hurt,
            5 => Self::Jump,
            6 => Self::Blip,
            _ => return None,
        })
    }
}

/// `rnd` and `frnd`: a uniform integer from 0 to `n`, and `x` times one of
/// 10001 steps from 0 to 1.
struct Dice<'a>(&'a mut Prg);

impl Dice<'_> {
    fn rnd(&mut self, n: u32) -> u32 {
        self.0.rand() % (n + 1)
    }

    fn yes(&mut self) -> bool {
        self.rnd(1) != 0
    }

    fn frnd(&mut self, x: f32) -> f32 {
        (self.0.rand() % 10001) as f32 / 10000.0 * x
    }
}

/// The settings of `preset`, from the defaults and `rand` already seeded.
pub(crate) fn preset(preset: SfxrPreset, rand: &mut Prg) -> SfxrParams {
    let mut p = SfxrParams::default();
    let mut d = Dice(rand);
    match preset {
        SfxrPreset::Coin => {
            p.base_freq = 0.4 + d.frnd(0.5);
            p.env_attack = 0.0;
            p.env_sustain = d.frnd(0.1);
            p.env_decay = 0.1 + d.frnd(0.4);
            p.env_punch = 0.3 + d.frnd(0.3);
            if d.yes() {
                p.arp_speed = 0.5 + d.frnd(0.2);
                p.arp_mod = 0.2 + d.frnd(0.4);
            }
        }
        SfxrPreset::Laser => laser(&mut p, &mut d),
        SfxrPreset::Explosion => explosion(&mut p, &mut d),
        SfxrPreset::Powerup => {
            if d.yes() {
                p.wave_type = 1;
            } else {
                p.duty = d.frnd(0.6);
            }
            if d.yes() {
                p.base_freq = 0.2 + d.frnd(0.3);
                p.freq_ramp = 0.1 + d.frnd(0.4);
                p.repeat_speed = 0.4 + d.frnd(0.4);
            } else {
                p.base_freq = 0.2 + d.frnd(0.3);
                p.freq_ramp = 0.05 + d.frnd(0.2);
                if d.yes() {
                    p.vib_strength = d.frnd(0.7);
                    p.vib_speed = d.frnd(0.6);
                }
            }
            p.env_attack = 0.0;
            p.env_sustain = d.frnd(0.4);
            p.env_decay = 0.1 + d.frnd(0.4);
        }
        SfxrPreset::Hurt => {
            p.wave_type = d.rnd(2) as i32;
            if p.wave_type == 2 {
                p.wave_type = 3;
            }
            if p.wave_type == 0 {
                p.duty = d.frnd(0.6);
            }
            p.base_freq = 0.2 + d.frnd(0.6);
            p.freq_ramp = -0.3 - d.frnd(0.4);
            p.env_attack = 0.0;
            p.env_sustain = d.frnd(0.1);
            p.env_decay = 0.1 + d.frnd(0.2);
            if d.yes() {
                p.hpf_freq = d.frnd(0.3);
            }
        }
        SfxrPreset::Jump => {
            p.wave_type = 0;
            p.duty = d.frnd(0.6);
            p.base_freq = 0.3 + d.frnd(0.3);
            p.freq_ramp = 0.1 + d.frnd(0.2);
            p.env_attack = 0.0;
            p.env_sustain = 0.1 + d.frnd(0.3);
            p.env_decay = 0.1 + d.frnd(0.2);
            if d.yes() {
                p.hpf_freq = d.frnd(0.3);
            }
            if d.yes() {
                p.lpf_freq = 1.0 - d.frnd(0.6);
            }
        }
        SfxrPreset::Blip => {
            p.wave_type = d.rnd(1) as i32;
            if p.wave_type == 0 {
                p.duty = d.frnd(0.6);
            }
            p.base_freq = 0.2 + d.frnd(0.4);
            p.env_attack = 0.0;
            p.env_sustain = 0.1 + d.frnd(0.1);
            p.env_decay = d.frnd(0.2);
            p.hpf_freq = 0.1;
        }
    }
    p
}

fn laser(p: &mut SfxrParams, d: &mut Dice<'_>) {
    p.wave_type = d.rnd(2) as i32;
    if p.wave_type == 2 && d.yes() {
        p.wave_type = d.rnd(1) as i32;
    }
    p.base_freq = 0.5 + d.frnd(0.5);
    p.freq_limit = p.base_freq - 0.2 - d.frnd(0.6);
    if p.freq_limit < 0.2 {
        p.freq_limit = 0.2;
    }
    p.freq_ramp = -0.15 - d.frnd(0.2);
    if d.rnd(2) == 0 {
        p.base_freq = 0.3 + d.frnd(0.6);
        p.freq_limit = d.frnd(0.1);
        p.freq_ramp = -0.35 - d.frnd(0.3);
    }
    if d.yes() {
        p.duty = d.frnd(0.5);
        p.duty_ramp = d.frnd(0.2);
    } else {
        p.duty = 0.4 + d.frnd(0.5);
        p.duty_ramp = -d.frnd(0.7);
    }
    p.env_attack = 0.0;
    p.env_sustain = 0.1 + d.frnd(0.2);
    p.env_decay = d.frnd(0.4);
    if d.yes() {
        p.env_punch = d.frnd(0.3);
    }
    if d.rnd(2) == 0 {
        p.pha_offset = d.frnd(0.2);
        p.pha_ramp = -d.frnd(0.2);
    }
    if d.yes() {
        p.hpf_freq = d.frnd(0.3);
    }
}

fn explosion(p: &mut SfxrParams, d: &mut Dice<'_>) {
    p.wave_type = 3;
    if d.yes() {
        p.base_freq = 0.1 + d.frnd(0.4);
        p.freq_ramp = -0.1 + d.frnd(0.4);
    } else {
        p.base_freq = 0.2 + d.frnd(0.7);
        p.freq_ramp = -0.2 - d.frnd(0.2);
    }
    p.base_freq *= p.base_freq;
    if d.rnd(4) == 0 {
        p.freq_ramp = 0.0;
    }
    if d.rnd(2) == 0 {
        p.repeat_speed = 0.3 + d.frnd(0.5);
    }
    p.env_attack = 0.0;
    p.env_sustain = 0.1 + d.frnd(0.3);
    p.env_decay = d.frnd(0.5);
    if !d.yes() {
        p.pha_offset = -0.3 + d.frnd(0.9);
        p.pha_ramp = -d.frnd(0.3);
    }
    p.env_punch = 0.2 + d.frnd(0.6);
    if d.yes() {
        p.vib_strength = d.frnd(0.7);
        p.vib_speed = d.frnd(0.6);
    }
    if d.rnd(2) == 0 {
        p.arp_speed = 0.6 + d.frnd(0.3);
        p.arp_mod = 0.8 - d.frnd(1.6);
    }
}
