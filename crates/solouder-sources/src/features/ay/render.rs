// Ported from SoLoud src/audiosource/ay/sndbuffer.cpp and sndrender.cpp,
// SoLoud's copy of the sound resampling core for the Unreal Speccy
// project, "created under public domain license by SMT, jan.2006";
// altered for SoLouder.

//! The AY player's sample ring (`SNDBUFFER`) and the resampler that turns
//! a chip's output levels, changing at chip ticks, into 44100 Hz samples
//! through a 128-tap FIR filter at 64 times oversampling (`SNDRENDER`).
//! All arithmetic is 32-bit and wraps, as in the C++.

use std::sync::LazyLock;

/// Samples in the ring, a power of two.
pub(crate) const RING_SIZE: u32 = 8192;

/// The ring both chips add their samples into: each sample's left level in
/// the low 16 bits and its right level in the high 16 (`SNDBUFFER`).
#[derive(Debug)]
pub(crate) struct Ring {
    pub(crate) samples: Box<[u32; RING_SIZE as usize]>,
    pub(crate) read_position: u32,
}

impl Ring {
    pub(crate) fn new() -> Self {
        Self {
            samples: Box::new([0; RING_SIZE as usize]),
            read_position: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.read_position = 0;
        self.samples.fill(0);
    }

    /// Samples rendered and not read, as far as `render` has written
    /// (`count_single`).
    pub(crate) fn ready(&self, render: &Render) -> u32 {
        render.dstpos.wrapping_sub(self.read_position) & (RING_SIZE - 1)
    }

    /// Clears `count` samples after reading them (`samples_read`).
    pub(crate) fn samples_read(&mut self, count: u32) {
        for i in 0..count {
            self.samples[((self.read_position + i) & (RING_SIZE - 1)) as usize] = 0;
        }
        self.read_position = (self.read_position + count) & (RING_SIZE - 1);
    }
}

/// Oversampling: 2^6 render ticks a sample.
const TICK_FF: u32 = 6;
const TICK_F: u32 = 1 << TICK_FF;
/// Fixed-point precision of system ticks to render ticks.
const MULT_C: u32 = 12;
const FILTER_SUM_FULL: u32 = 0x10000;
const FILTER_SUM_HALF: u32 = 0x8000;

/// The FIR filter, "designed with Matlab's DSP toolbox".
#[allow(clippy::excessive_precision)]
const FILTER_COEFF: [f64; (TICK_F * 2) as usize] = [
    0.000797243121022152,
    0.000815206499600866,
    0.000844792477531490,
    0.000886460636664257,
    0.000940630171246217,
    0.001007677515787512,
    0.001087934129054332,
    0.001181684445143001,
    0.001289164001921830,
    0.001410557756409498,
    0.001545998595893740,
    0.001695566052785407,
    0.001859285230354019,
    0.002037125945605404,
    0.002229002094643918,
    0.002434771244914945,
    0.002654234457752337,
    0.002887136343664226,
    0.003133165351783907,
    0.003391954293894633,
    0.003663081102412781,
    0.003946069820687711,
    0.004240391822953223,
    0.004545467260249598,
    0.004860666727631453,
    0.005185313146989532,
    0.005518683858848785,
    0.005860012915564928,
    0.006208493567431684,
    0.006563280932335042,
    0.006923494838753613,
    0.007288222831108771,
    0.007656523325719262,
    0.008027428904915214,
    0.008399949736219575,
    0.008773077102914008,
    0.009145787031773989,
    0.009517044003286715,
    0.009885804729257883,
    0.010251021982371376,
    0.010611648461991030,
    0.010966640680287394,
    0.011314962852635887,
    0.011655590776166550,
    0.011987515680350414,
    0.012309748033583185,
    0.012621321289873522,
    0.012921295559959939,
    0.013208761191466523,
    0.013482842243062109,
    0.013742699838008606,
    0.013987535382970279,
    0.014216593638504731,
    0.014429165628265581,
    0.014624591374614174,
    0.014802262449059521,
    0.014961624326719471,
    0.015102178534818147,
    0.015223484586101132,
    0.015325161688957322,
    0.015406890226980602,
    0.015468413001680802,
    0.015509536233058410,
    0.015530130313785910,
    0.015530130313785910,
    0.015509536233058410,
    0.015468413001680802,
    0.015406890226980602,
    0.015325161688957322,
    0.015223484586101132,
    0.015102178534818147,
    0.014961624326719471,
    0.014802262449059521,
    0.014624591374614174,
    0.014429165628265581,
    0.014216593638504731,
    0.013987535382970279,
    0.013742699838008606,
    0.013482842243062109,
    0.013208761191466523,
    0.012921295559959939,
    0.012621321289873522,
    0.012309748033583185,
    0.011987515680350414,
    0.011655590776166550,
    0.011314962852635887,
    0.010966640680287394,
    0.010611648461991030,
    0.010251021982371376,
    0.009885804729257883,
    0.009517044003286715,
    0.009145787031773989,
    0.008773077102914008,
    0.008399949736219575,
    0.008027428904915214,
    0.007656523325719262,
    0.007288222831108771,
    0.006923494838753613,
    0.006563280932335042,
    0.006208493567431684,
    0.005860012915564928,
    0.005518683858848785,
    0.005185313146989532,
    0.004860666727631453,
    0.004545467260249598,
    0.004240391822953223,
    0.003946069820687711,
    0.003663081102412781,
    0.003391954293894633,
    0.003133165351783907,
    0.002887136343664226,
    0.002654234457752337,
    0.002434771244914945,
    0.002229002094643918,
    0.002037125945605404,
    0.001859285230354019,
    0.001695566052785407,
    0.001545998595893740,
    0.001410557756409498,
    0.001289164001921830,
    0.001181684445143001,
    0.001087934129054332,
    0.001007677515787512,
    0.000940630171246217,
    0.000886460636664257,
    0.000844792477531490,
    0.000815206499600866,
    0.000797243121022152,
];

/// The filter's running sums in 16.16 fixed point (`init_diff`).
static FILTER_DIFF: LazyLock<[u32; (TICK_F * 2) as usize]> = LazyLock::new(|| {
    let mut diff = [0; (TICK_F * 2) as usize];
    let mut sum = 0.0_f64;
    for (value, coeff) in diff.iter_mut().zip(FILTER_COEFF) {
        *value = (sum * 65536.0) as i32 as u32;
        sum += coeff;
    }
    diff
});

/// Forces the filter table, so the audio thread never builds it.
pub(crate) fn init_tables() {
    LazyLock::force(&FILTER_DIFF);
}

/// One chip's resampler (`SNDRENDER`), writing into a shared [`Ring`].
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Render {
    pub(crate) mix_l: u32,
    pub(crate) mix_r: u32,
    pub(crate) dstpos: u32,
    dst_start: u32,
    tick: u32,
    base_tick: u32,
    s1_l: u32,
    s1_r: u32,
    s2_l: u32,
    s2_r: u32,
    sys_clock_rate: u32,
    sample_rate: u32,
    passed_sys_ticks: u32,
    passed_snd_ticks: u32,
    mult_const: u32,
}

impl Render {
    pub(crate) fn set_timings(&mut self, sys_clock_rate: u32, sample_rate: u32) {
        self.sys_clock_rate = sys_clock_rate;
        self.sample_rate = sample_rate;
        self.tick = 0;
        self.dstpos = 0;
        self.dst_start = 0;
        self.passed_snd_ticks = 0;
        self.passed_sys_ticks = 0;
        self.mult_const =
            ((i64::from(sample_rate) << (MULT_C + TICK_FF)) / i64::from(sys_clock_rate)) as u32;
    }

    pub(crate) fn start_frame(&mut self) {
        self.dst_start = self.dstpos;
        self.base_tick = self.tick;
    }

    /// The levels change at `sys_tick` of the frame.
    pub(crate) fn update(&mut self, ring: &mut Ring, sys_tick: u32, l: u32, r: u32) {
        if (l ^ self.mix_l) | (r ^ self.mix_r) == 0 {
            return;
        }
        let endtick = sys_tick.wrapping_mul(self.mult_const) >> MULT_C;
        self.flush(ring, self.base_tick.wrapping_add(endtick));
        self.mix_l = l;
        self.mix_r = r;
    }

    /// Renders the levels to the frame's end, `end_sys_tick`.
    pub(crate) fn end_frame(&mut self, ring: &mut Ring, end_sys_tick: u32) {
        let endtick = (i64::from(self.passed_sys_ticks.wrapping_add(end_sys_tick))
            * i64::from(self.sample_rate)
            * i64::from(TICK_F)
            / i64::from(self.sys_clock_rate)) as u32;
        self.flush(ring, endtick.wrapping_sub(self.passed_snd_ticks));
        self.passed_sys_ticks = self.passed_sys_ticks.wrapping_add(end_sys_tick);
        let mut ready = self.dstpos.wrapping_sub(self.dst_start);
        if (ready as i32) < 0 {
            ready = ready.wrapping_add(RING_SIZE);
        }
        self.tick = self.tick.wrapping_sub(ready << TICK_FF);
        self.passed_snd_ticks = self.passed_snd_ticks.wrapping_add(ready << TICK_FF);
    }

    fn put(&mut self, ring: &mut Ring, value: u32) {
        let slot = &mut ring.samples[self.dstpos as usize];
        *slot = slot.wrapping_add(value);
        self.dstpos = (self.dstpos + 1) & (RING_SIZE - 1);
    }

    /// Renders the current levels from `tick` to `endtick` (`flush`).
    fn flush(&mut self, ring: &mut Ring, endtick: u32) {
        let diff = &*FILTER_DIFF;
        let frac = |tick: u32| (tick & (TICK_F - 1)) as usize;
        let (l, r) = (self.mix_l, self.mix_r);
        if (endtick ^ self.tick) & !(TICK_F - 1) == 0 {
            let scale = diff[frac(endtick) + TICK_F as usize]
                .wrapping_sub(diff[frac(self.tick) + TICK_F as usize]);
            self.s2_l = self.s2_l.wrapping_add(l.wrapping_mul(scale));
            self.s2_r = self.s2_r.wrapping_add(r.wrapping_mul(scale));
            let scale = diff[frac(endtick)].wrapping_sub(diff[frac(self.tick)]);
            self.s1_l = self.s1_l.wrapping_add(l.wrapping_mul(scale));
            self.s1_r = self.s1_r.wrapping_add(r.wrapping_mul(scale));
            self.tick = endtick;
            return;
        }
        let scale = FILTER_SUM_FULL.wrapping_sub(diff[frac(self.tick) + TICK_F as usize]);
        let value = (l.wrapping_mul(scale).wrapping_add(self.s2_l) >> 16)
            .wrapping_add(r.wrapping_mul(scale).wrapping_add(self.s2_r) & 0xFFFF_0000);
        self.put(ring, value);
        let scale = FILTER_SUM_HALF.wrapping_sub(diff[frac(self.tick)]);
        self.s2_l = self.s1_l.wrapping_add(l.wrapping_mul(scale));
        self.s2_r = self.s1_r.wrapping_add(r.wrapping_mul(scale));
        self.tick = (self.tick | (TICK_F - 1)).wrapping_add(1);
        if (endtick ^ self.tick) & !(TICK_F - 1) != 0 {
            // The filter is symmetric.
            let val_l = l.wrapping_mul(FILTER_SUM_HALF);
            let val_r = r.wrapping_mul(FILTER_SUM_HALF);
            loop {
                let value = (self.s2_l.wrapping_add(val_l) >> 16)
                    .wrapping_add(self.s2_r.wrapping_add(val_r) & 0xFFFF_0000);
                self.put(ring, value);
                self.tick = self.tick.wrapping_add(TICK_F);
                self.s2_l = val_l;
                self.s2_r = val_r;
                if (endtick ^ self.tick) & !(TICK_F - 1) == 0 {
                    break;
                }
            }
        }
        self.tick = endtick;
        let scale = diff[frac(endtick) + TICK_F as usize].wrapping_sub(FILTER_SUM_HALF);
        self.s2_l = self.s2_l.wrapping_add(l.wrapping_mul(scale));
        self.s2_r = self.s2_r.wrapping_add(r.wrapping_mul(scale));
        let scale = diff[frac(endtick)];
        self.s1_l = l.wrapping_mul(scale);
        self.s1_r = r.wrapping_mul(scale);
    }
}
