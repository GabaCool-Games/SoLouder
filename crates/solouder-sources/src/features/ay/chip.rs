// Ported from SoLoud src/audiosource/ay/sndchip.cpp and sndchip.h, SoLoud's
// copy of the YM-2149F emulator for the Unreal Speccy project, "created
// under public domain license by SMT, jan.2006"; altered for SoLouder.

//! The AY-3-8910 and YM2149F sound chip (`SNDCHIP`): three square-wave
//! tone generators, a noise generator and an envelope, ticking at the
//! chip's clock / 8, each change of its output levels sent to a
//! [`Render`]. All arithmetic is 32-bit and wraps, as in the C++.

use super::render::{Render, Ring};

/// Output levels for each 5-bit envelope or volume value (`SNDCHIP_VOLTAB`).
type VolumeTable = [u32; 32];

/// `SNDR_VOL_AY`: the AY has 16 levels, each used twice.
const VOL_AY: VolumeTable = [
    0x0000, 0x0000, 0x0340, 0x0340, 0x04C0, 0x04C0, 0x06F2, 0x06F2, 0x0A44, 0x0A44, 0x0F13, 0x0F13,
    0x1510, 0x1510, 0x227E, 0x227E, 0x289F, 0x289F, 0x414E, 0x414E, 0x5B21, 0x5B21, 0x7258, 0x7258,
    0x905E, 0x905E, 0xB550, 0xB550, 0xD7A0, 0xD7A0, 0xFFFF, 0xFFFF,
];

/// `SNDR_VOL_YM`: the YM's envelope has 32 levels.
const VOL_YM: VolumeTable = [
    0x0000, 0x0000, 0x00EF, 0x01D0, 0x0290, 0x032A, 0x03EE, 0x04D2, 0x0611, 0x0782, 0x0912, 0x0A36,
    0x0C31, 0x0EB6, 0x1130, 0x13A0, 0x1751, 0x1BF5, 0x20E2, 0x2594, 0x2CA1, 0x357F, 0x3E45, 0x475E,
    0x5502, 0x6620, 0x7730, 0x8844, 0xA1D2, 0xC102, 0xE0A2, 0xFFFF,
];

/// `SNDR_PAN_ABC`: percent left and right for channels A, B and C.
const PAN_ABC: [u32; 6] = [100, 10, 66, 66, 10, 100];

/// Fixed-point precision of system ticks to chip ticks.
const MULT_C_1: u32 = 14;

/// One AY or YM chip (`SNDCHIP`).
#[derive(Clone, Debug)]
pub(crate) struct Chip {
    pub(crate) render: Render,
    ym: bool,
    t: u32,
    ta: u32,
    tb: u32,
    tc: u32,
    tn: u32,
    te: u32,
    env: u32,
    denv: u32,
    bit_a: u32,
    bit_b: u32,
    bit_c: u32,
    bit_n: u32,
    ns: u32,
    /// Register 7's tone and noise disable bits, each 0 or all ones.
    bits: [u32; 6],
    ea: u32,
    eb: u32,
    ec: u32,
    va: u32,
    vb: u32,
    vc: u32,
    fa: u32,
    fb: u32,
    fc: u32,
    fn_: u32,
    fe: u32,
    mult_const: u32,
    active_reg: u8,
    vols: [[u32; 32]; 6],
    /// Registers 0 to 15 (`reg`, `AYREGS`).
    pub(crate) reg: [u8; 16],
    chip_clock_rate: u32,
    system_clock_rate: u32,
    passed_chip_ticks: u32,
    passed_clk_ticks: u32,
}

impl Chip {
    /// A chip as `ChipPlayer` sets it up: everything at 0 (reference patch
    /// 0083), timed to `system_clock_rate` ticks and a `chip_clock_rate`
    /// clock, the AY's or the YM's levels at half volume, panned ABC.
    pub(crate) fn new(system_clock_rate: u32, chip_clock_rate: u32, ym: bool) -> Self {
        let mut chip = Self {
            render: Render::default(),
            ym,
            t: 0,
            ta: 0,
            tb: 0,
            tc: 0,
            tn: 0,
            te: 0,
            env: 0,
            denv: 0,
            bit_a: 0,
            bit_b: 0,
            bit_c: 0,
            bit_n: 0,
            ns: 0,
            bits: [0; 6],
            ea: 0,
            eb: 0,
            ec: 0,
            va: 0,
            vb: 0,
            vc: 0,
            fa: 0,
            fb: 0,
            fc: 0,
            fn_: 0,
            fe: 0,
            mult_const: 0,
            active_reg: 0,
            vols: [[0; 32]; 6],
            reg: [0; 16],
            chip_clock_rate: 0,
            system_clock_rate: 0,
            passed_chip_ticks: 0,
            passed_clk_ticks: 0,
        };
        chip.set_timings(system_clock_rate, chip_clock_rate, 44100);
        // VOL_DIV halves the volume: two chips add into one sample.
        chip.set_volumes(0x7FFF / 2, if ym { &VOL_YM } else { &VOL_AY });
        chip
    }

    fn set_timings(&mut self, system_clock_rate: u32, chip_clock_rate: u32, sample_rate: u32) {
        let chip_clock_rate = chip_clock_rate / 8;
        self.system_clock_rate = system_clock_rate;
        self.chip_clock_rate = chip_clock_rate;
        self.mult_const =
            ((i64::from(chip_clock_rate) << MULT_C_1) / i64::from(system_clock_rate)) as u32;
        self.render.set_timings(chip_clock_rate, sample_rate);
        self.passed_chip_ticks = 0;
        self.passed_clk_ticks = 0;
        self.t = 0;
        self.ns = 0xFFFF;
        self.apply_regs();
    }

    fn set_volumes(&mut self, global_vol: u32, table: &VolumeTable) {
        for (vols, pan) in self.vols.iter_mut().zip(PAN_ABC) {
            for (vol, level) in vols.iter_mut().zip(table) {
                *vol = (i64::from(global_vol) * i64::from(*level) * i64::from(pan)
                    / (65535 * 100 * 3)) as u32;
            }
        }
    }

    /// Writes every register to another value and back, so the values
    /// derived from them are set (`apply_regs`, at timestamp 0).
    fn apply_regs(&mut self) {
        for r in 0..16 {
            self.select(r);
            let p = self.reg[r as usize];
            for val in [p ^ 1, p] {
                if let Some(val) = self.masked(val) {
                    self.reg[usize::from(self.active_reg)] = val;
                    self.derive(val);
                }
            }
        }
    }

    pub(crate) fn start_frame(&mut self) {
        self.render.start_frame();
    }

    pub(crate) fn end_frame(&mut self, ring: &mut Ring, clk_ticks: u32) {
        let end_chip_tick = ((i64::from(self.passed_clk_ticks) + i64::from(clk_ticks))
            * i64::from(self.chip_clock_rate)
            / i64::from(self.system_clock_rate)) as u32;
        self.flush(ring, end_chip_tick.wrapping_sub(self.passed_chip_ticks));
        self.render.end_frame(ring, self.t);
        self.passed_clk_ticks = self.passed_clk_ticks.wrapping_add(clk_ticks);
        self.passed_chip_ticks = self.passed_chip_ticks.wrapping_add(self.t);
        self.t = 0;
    }

    pub(crate) fn select(&mut self, reg: u8) {
        self.active_reg = if self.ym { reg } else { reg & 0x0F };
    }

    /// `val` masked for the selected register, or `None` if the register
    /// does not exist or keeps its value (register 13 always restarts the
    /// envelope).
    fn masked(&self, val: u8) -> Option<u8> {
        let reg = self.active_reg;
        if reg >= 0x10 {
            return None;
        }
        let bit = 1_u32 << reg;
        let val = if bit & (1 << 1 | 1 << 3 | 1 << 5 | 1 << 13) != 0 {
            val & 0x0F
        } else if bit & (1 << 6 | 1 << 8 | 1 << 9 | 1 << 10) != 0 {
            val & 0x1F
        } else {
            val
        };
        (reg == 13 || self.reg[usize::from(reg)] != val).then_some(val)
    }

    /// Writes `val` to the selected register at `timestamp` system ticks
    /// into the frame (`write`). The chip runs up to then first, with the
    /// new register value but the old derived ones, as in the C++.
    pub(crate) fn write(&mut self, ring: &mut Ring, timestamp: u32, val: u8) {
        let Some(val) = self.masked(val) else {
            return;
        };
        self.reg[usize::from(self.active_reg)] = val;
        if timestamp != 0 {
            self.flush(ring, timestamp.wrapping_mul(self.mult_const) >> MULT_C_1);
        }
        self.derive(val);
    }

    fn word(&self, reg: usize) -> u32 {
        u32::from(self.reg[reg]) | u32::from(self.reg[reg + 1]) << 8
    }

    /// The values derived from the register just written.
    fn derive(&mut self, val: u8) {
        let mask = |bit: u8| 0_u32.wrapping_sub(u32::from(val >> bit) & 1);
        let envelope = |val: u8| if val & 0x10 != 0 { u32::MAX } else { 0 };
        let volume = |val: u8, e: u32| ((u32::from(val) & 0x0F) * 2 + 1) & !e;
        match self.active_reg {
            0 | 1 => self.fa = self.word(0),
            2 | 3 => self.fb = self.word(2),
            4 | 5 => self.fc = self.word(4),
            6 => self.fn_ = u32::from(val) * 2,
            7 => {
                for (bit, value) in self.bits.iter_mut().enumerate() {
                    *value = mask(bit as u8);
                }
            }
            8 => {
                self.ea = envelope(val);
                self.va = volume(val, self.ea);
            }
            9 => {
                self.eb = envelope(val);
                self.vb = volume(val, self.eb);
            }
            10 => {
                self.ec = envelope(val);
                self.vc = volume(val, self.ec);
            }
            11 | 12 => self.fe = self.word(11),
            13 => {
                self.te = 0;
                if self.reg[13] & 4 != 0 {
                    // Attack.
                    self.env = 0;
                    self.denv = 1;
                } else {
                    // Decay.
                    self.env = 31;
                    self.denv = u32::MAX;
                }
            }
            _ => {}
        }
    }

    /// Runs the chip up to `chiptick` chip ticks into the frame (`flush`).
    fn flush(&mut self, ring: &mut Ring, chiptick: u32) {
        while self.t < chiptick {
            self.t += 1;
            self.ta += 1;
            if self.ta >= self.fa {
                self.ta = 0;
                self.bit_a ^= u32::MAX;
            }
            self.tb += 1;
            if self.tb >= self.fb {
                self.tb = 0;
                self.bit_b ^= u32::MAX;
            }
            self.tc += 1;
            if self.tc >= self.fc {
                self.tc = 0;
                self.bit_c ^= u32::MAX;
            }
            self.tn += 1;
            if self.tn >= self.fn_ {
                self.tn = 0;
                let ns = self.ns;
                self.ns = ns.wrapping_mul(2).wrapping_add(1) ^ (((ns >> 16) ^ (ns >> 13)) & 1);
                self.bit_n = 0_u32.wrapping_sub((self.ns >> 16) & 1);
            }
            self.te += 1;
            if self.te >= self.fe {
                self.te = 0;
                self.step_envelope();
            }
            let [b0, b1, b2, b3, b4, b5] = self.bits;
            let level = |e: u32, v: u32, bit: u32, tone: u32, noise: u32, env: u32| {
                ((e & env) | v) & ((bit | tone) & (self.bit_n | noise))
            };
            let en_a = level(self.ea, self.va, self.bit_a, b0, b3, self.env) as usize;
            let en_b = level(self.eb, self.vb, self.bit_b, b1, b4, self.env) as usize;
            let en_c = level(self.ec, self.vc, self.bit_c, b2, b5, self.env) as usize;
            let v = &self.vols;
            let l = v[0][en_a].wrapping_add(v[2][en_b]).wrapping_add(v[4][en_c]);
            let r = v[1][en_a].wrapping_add(v[3][en_b]).wrapping_add(v[5][en_c]);
            if (l ^ self.render.mix_l) | (r ^ self.render.mix_r) != 0 {
                self.render.update(ring, self.t, l, r);
            }
        }
    }

    /// One envelope step, and what each shape does past its end.
    fn step_envelope(&mut self) {
        self.env = self.env.wrapping_add(self.denv);
        if self.env & !31 == 0 {
            return;
        }
        let mask = 1_u32 << self.reg[13];
        if mask & (0xFF | 1 << 9 | 1 << 15) != 0 {
            self.env = 0;
            self.denv = 0;
        } else if mask & (1 << 8 | 1 << 12) != 0 {
            self.env &= 31;
        } else if mask & (1 << 10 | 1 << 14) != 0 {
            self.denv = (self.denv as i32).wrapping_neg() as u32;
            self.env = self.env.wrapping_add(self.denv);
        } else {
            // Shapes 11 and 13.
            self.env = 31;
            self.denv = 0;
        }
    }
}
