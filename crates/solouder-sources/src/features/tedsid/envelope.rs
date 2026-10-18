// Ported from SoLoud src/audiosource/tedsid/sid.cpp, SoLoud's copy of
// tedplay, (c) 2012 Attila Grosz, used under the Unlicense
// (http://unlicense.org/); altered for SoLouder.

//! A SID voice's envelope generator (`SIDsound::doEnvelopeGenerator`),
//! after Kevin Horton's notes: a 15-bit LFSR counts each rate's period, and
//! decay and release steps are spaced out by level for an exponential
//! curve.

/// The LFSR value that ends each rate's period.
const RATE_COUNT_PERIOD: [u32; 16] = [
    0x7F00, 0x0006, 0x003C, 0x0330, 0x20C0, 0x6755, 0x3800, 0x500E, 0x1212, 0x0222, 0x1848, 0x59B8,
    0x3840, 0x77E2, 0x7625, 0x0A93,
];

/// Periods between decay and release steps, by level.
const DR_DIVISORS: [u8; 256] = {
    let mut table = [1; 256];
    let mut i = 0;
    while i < 93 {
        table[i] = match i {
            0..=5 => 30,
            6..=13 => 16,
            14..=25 => 8,
            26..=53 => 4,
            _ => 2,
        };
        i += 1;
    }
    table
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Frozen,
    Attack,
    Decay,
    Release,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Envelope {
    state: State,
    attack: u32,
    decay: u32,
    sustain: u32,
    release: u32,
    /// 8 bits (reference patch 0081; SoLoud's attack counted on to 0x1FF).
    level: u32,
    counter: u32,
    exp_counter: u32,
    /// An index into [`RATE_COUNT_PERIOD`] (reference patch 0082; SoLoud
    /// started it at the first period itself).
    rate: u32,
    gate: u32,
}

impl Envelope {
    pub(crate) fn new() -> Self {
        Self {
            state: State::Frozen,
            attack: 0,
            decay: 0,
            sustain: 0,
            release: 0,
            level: 0,
            counter: 0x7fff,
            exp_counter: 0,
            rate: 0,
            gate: 0,
        }
    }

    /// The control register's gate bit.
    pub(crate) fn set_gate(&mut self, gate: u32) {
        if gate == self.gate {
            return;
        }
        if gate != 0 {
            self.state = State::Attack;
            self.rate = self.attack;
        } else {
            self.state = State::Release;
            self.rate = self.release;
        }
        self.gate = gate;
    }

    pub(crate) fn set_attack_decay(&mut self, value: u8) {
        self.attack = u32::from(value >> 4);
        self.decay = u32::from(value & 0x0F);
        match self.state {
            State::Attack => self.rate = self.attack,
            State::Decay => self.rate = self.decay,
            _ => {}
        }
    }

    pub(crate) fn set_sustain_release(&mut self, value: u8) {
        self.sustain = u32::from(value >> 4) * 0x11;
        self.release = u32::from(value & 0x0F);
        if self.state == State::Release {
            self.rate = self.release;
        }
    }

    /// The level, 0 to 255 (the SID's register 0x1C for voice 3).
    #[cfg(test)]
    pub(crate) fn level(&self) -> u32 {
        self.level
    }

    /// Runs `cycles` cycles, at least 1, and returns the level.
    pub(crate) fn run(&mut self, cycles: u32) -> u32 {
        for _ in 0..cycles.max(1) {
            if self.counter != RATE_COUNT_PERIOD[self.rate as usize] {
                let feedback = ((self.counter >> 14) ^ (self.counter >> 13)) & 1;
                self.counter = ((self.counter << 1) | feedback) & 0x7FFF;
                continue;
            }
            self.counter = 0x7fff;
            if self.state != State::Attack {
                self.exp_counter = self.exp_counter.wrapping_add(1);
                if self.exp_counter != u32::from(DR_DIVISORS[self.level as usize]) {
                    continue;
                }
            }
            self.exp_counter = 0;
            match self.state {
                State::Attack => {
                    self.level = (self.level + 1) & 0xFF;
                    if self.level == 0xFF {
                        self.state = State::Decay;
                        self.rate = self.decay;
                    }
                }
                State::Decay => {
                    if self.level != self.sustain {
                        self.level = self.level.wrapping_sub(1) & 0xFF;
                        if self.level == 0 {
                            self.state = State::Frozen;
                        }
                    }
                }
                State::Release => {
                    self.level = self.level.wrapping_sub(1) & 0xFF;
                    if self.level == 0 {
                        self.state = State::Frozen;
                    }
                }
                State::Frozen => self.level = 0,
            }
        }
        self.level
    }
}
