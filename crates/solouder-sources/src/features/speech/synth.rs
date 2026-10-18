// Ported from SoLoud src/audiosource/speech/klatt.cpp, SoLoud's copy of
// rsynth by Nick Ing-Simmons et al. (public domain; SoLoud's changes public
// domain or CC0, Jari Komppa 2013); altered for SoLouder.

//! Elements to frames (`klatt::initsynth` and `klatt::synth`): each
//! element's parameters move to and from its neighbours', and the pitch
//! follows the stress.

use solouder_core::dsp::c_int;

use super::elements::{self, Element, PARAMS};
use super::klatt::Klatt;

/// A transition's boundary value and length in frames (`Slope`).
#[derive(Clone, Copy, Debug, Default)]
struct Slope {
    value: f32,
    time: i32,
}

/// Where the synthesiser is in the elements.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SynthState {
    count: usize,
    index: usize,
    last: usize,
    t_stress: i32,
    nt_stress: i32,
    stress_start: Slope,
    stress_end: Slope,
    top: f32,
}

/// The transitions from `b` into or out of `a`, which dominates
/// (`set_trans`); `ext` takes `a`'s external delays.
fn set_trans(a: &Element, b: &Element, ext: bool) -> [Slope; PARAMS] {
    std::array::from_fn(|i| {
        let ai = &a.interp[i];
        let time = if ext { ai.ext_delay } else { ai.int_delay };
        let value = if time != 0 {
            ai.fixed + (ai.proportion as f32 * b.interp[i].steady) * 0.01
        } else {
            b.interp[i].steady
        };
        Slope { value, time }
    })
}

fn lerp(a: f32, b: f32, t: i32, d: i32) -> f32 {
    if t <= 0 {
        return a;
    }
    if t >= d {
        return b;
    }
    let f = t as f32 / d as f32;
    a + (b - a) * f
}

fn interpolate(start: Slope, end: Slope, mid: f32, time: i32, duration: i32) -> f32 {
    let steady = duration - (start.time + end.time);
    if steady >= 0 {
        // To the midpoint, steady for a while, then to the end.
        if time < start.time {
            return lerp(start.value, mid, time, start.time);
        }
        let time = time - start.time;
        if time <= steady {
            return mid;
        }
        lerp(mid, end.value, time - steady, end.time)
    } else {
        let f = 1.0 - (time as f32 / duration as f32);
        let sp = lerp(start.value, mid, time, start.time);
        let ep = lerp(end.value, mid, duration - time, end.time);
        f * sp + (1.0 - f) * ep
    }
}

/// `AMP_ADJ`: raises the formant amplitudes to Klatt's levels.
const AMP_ADJ: i32 = 14;

impl Klatt {
    /// Starts the elements (`initsynth`).
    pub(super) fn init_synth(&mut self, count: usize) {
        let table = elements::elements();
        let s = &mut self.synth;
        s.count = count;
        s.index = 0;
        s.last = 0;
        s.t_stress = 0;
        s.nt_stress = 0;
        s.stress_start.time = 40;
        s.stress_end.time = 40;
        s.stress_end.value = 0.0;
        self.seed = 5;
        self.frame.f0 = self.base_f0;
        s.top = 1.1 * self.frame.f0 as f32;
        self.frame.nasal_pole_freq = c_int(f64::from(table[0].interp[elements::FN].steady));
        self.frame.parallel_bandwidth[..3].copy_from_slice(&[60, 90, 150]);
    }

    /// The next element's frames into `out` (`synth`): the number of
    /// samples, 0 for an element of no frames, or -1 after the last.
    /// `out` holds the longest element's frames.
    pub(super) fn synth(&mut self, elements: &[u8], out: &mut [i16]) -> i32 {
        let table = elements::elements();
        let element = |at: usize| &table[usize::from(elements.get(at).copied().unwrap_or(0))];
        let byte = |at: usize| i32::from(elements.get(at).copied().unwrap_or(0));
        let count = self.synth.count;
        if self.synth.index >= count {
            return -1;
        }
        let current_index = usize::from(elements[self.synth.index]);
        let current = &table[current_index];
        let duration = byte(self.synth.index + 1);
        self.synth.index += 3; // the stress is skipped
        if current.rank == 31 {
            // END: the pitch starts over.
            self.frame.f0 = self.base_f0;
            self.synth.top = 1.1 * self.frame.f0 as f32;
        }
        let frame_samples = self.frame_samples.max(0) as usize;
        let mut written = 0;
        // Elements of no frames only set their neighbours' boundaries.
        if duration > 0 {
            let index = self.synth.index;
            let next = if index < count {
                element(index)
            } else {
                &table[0]
            };
            let last = &table[self.synth.last];
            let start = if current.rank > last.rank {
                set_trans(current, last, false)
            } else {
                set_trans(last, current, true)
            };
            let end = if next.rank > current.rank {
                set_trans(next, current, true)
            } else {
                set_trans(current, next, false)
            };
            for t in 0..duration {
                let base = self.synth.top * 0.8;
                if self.synth.t_stress == self.synth.nt_stress {
                    self.next_stress(elements, duration);
                }
                let tp: [f32; PARAMS] = std::array::from_fn(|j| {
                    interpolate(start[j], end[j], current.interp[j].steady, t, duration)
                });
                let s = &self.synth;
                let contour =
                    interpolate(s.stress_start, s.stress_end, 0.0, s.t_stress, s.nt_stress);
                let f = &mut self.frame;
                f.f0 = c_int(f64::from(base + (s.top - base) * contour));
                let int = |j: usize| c_int(f64::from(tp[j]));
                f.voicing_db = int(elements::AV);
                f.frication_db = int(elements::AF);
                f.nasal_zero_freq = int(elements::FN);
                f.aspiration_db = int(elements::ASP);
                f.breathiness = int(elements::AVC);
                f.parallel_bandwidth[0] = int(elements::B1);
                f.parallel_bandwidth[1] = int(elements::B2);
                f.parallel_bandwidth[2] = int(elements::B3);
                f.formant_freq[0] = int(elements::F1);
                f.formant_freq[1] = int(elements::F2);
                f.formant_freq[2] = int(elements::F3);
                f.bypass_db = AMP_ADJ.wrapping_add(int(elements::AB));
                for (k, param) in [elements::A1, elements::A2, elements::A3, elements::A4]
                    .into_iter()
                    .chain([elements::A5, elements::A6])
                    .enumerate()
                {
                    f.formant_db[k] = AMP_ADJ.wrapping_add(int(param));
                }
                if let Some(frame) = out.get_mut(written..written + frame_samples) {
                    self.parwave(frame);
                }
                written += frame_samples;
                // Declination of the pitch, 0.25 Hz per frame.
                self.synth.top -= self.base_declination;
                self.synth.t_stress += 1;
            }
        }
        self.synth.last = current_index;
        written as i32
    }

    /// Finds the next stressed element or vowel ahead, setting the stress
    /// contour's end and length. Past the elements, an element reads as
    /// END with stress 3 and a duration as 0 (reference patch 0102).
    fn next_stress(&mut self, elements: &[u8], duration: i32) {
        let table = elements::elements();
        let count = self.synth.count;
        let s = &mut self.synth;
        let mut j = s.index;
        s.stress_start = s.stress_end;
        s.t_stress = 0;
        s.nt_stress = duration;
        let take = |j: &mut usize, past: u8| -> u8 {
            if *j < count {
                *j += 1;
                elements[*j - 1]
            } else {
                past
            }
        };
        while j <= count {
            let e = &table[usize::from(take(&mut j, 0))];
            let mut du = i32::from(take(&mut j, 0));
            let stress = take(&mut j, 3);
            if stress != 0 || e.vowel {
                let mut d = 0;
                s.stress_end.value = if stress != 0 {
                    f32::from(stress) / 3.0
                } else {
                    0.1
                };
                loop {
                    d += du;
                    let e = &table[usize::from(take(&mut j, 0))];
                    du = i32::from(take(&mut j, 0));
                    // A vowel was read inside the elements, so its stress is.
                    if !e.vowel || take(&mut j, 0) != stress {
                        break;
                    }
                }
                s.nt_stress += d / 2;
                break;
            }
            s.nt_stress += du;
        }
    }
}
