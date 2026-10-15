// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! Decoding one audio packet: `vorbis_decode_initial` (packet type, mode
//! and windowing), `vorbis_decode_packet_rest` (floors, residues, inverse
//! coupling, floor rendering, inverse MDCT and the sample position) and
//! `vorbis_decode_packet`.

use super::decoder::Vorbis;
use super::floor::{decode_floor1, do_floor};
use super::imdct::inverse_mdct;
use super::ogg::{EOP, PAGEFLAG_LAST_PAGE};
use super::residue::{Buffers, decode_residue};
use super::setup::Mode;
use super::source::Source;
use super::tables::ilog;

/// What `vorbis_decode_initial` reports: the window's `left_start`,
/// `left_end`, `right_start` and `right_end`, and the mode.
#[derive(Clone, Copy, Debug)]
pub(super) struct Initial {
    pub left_start: i32,
    #[allow(dead_code)] // reported as the C does; the pull path ignores it
    pub left_end: i32,
    pub right_start: i32,
    pub right_end: i32,
    pub mode: usize,
}

/// Two different channel buffers of `stride` values.
fn two_channels(data: &mut [f32], stride: usize, a: usize, b: usize) -> (&mut [f32], &mut [f32]) {
    if a < b {
        let (low, high) = data.split_at_mut(b * stride);
        (&mut low[a * stride..][..stride], &mut high[..stride])
    } else {
        let (low, high) = data.split_at_mut(a * stride);
        (&mut high[..stride], &mut low[b * stride..][..stride])
    }
}

impl<S: Source> Vorbis<S> {
    /// `vorbis_decode_initial`: starts the next audio packet (skipping other
    /// packets) and reads its mode and window shape.
    pub fn decode_initial(&mut self) -> Option<Initial> {
        self.channel_buffer_start = 0;
        self.channel_buffer_end = 0;
        let f = &mut self.r;
        loop {
            if f.eof || !f.maybe_start_packet() {
                return None;
            }
            // check packet type
            if f.get_bits(1) == 0 {
                break;
            }
            while f.get8_packet() != EOP {}
        }
        let mode_count = self.s.modes.len() as i32;
        let i = f.get_bits(ilog(mode_count - 1)) as i32;
        if i >= mode_count {
            return None;
        }
        let m = self.s.modes[i as usize];
        let (n, prev, next) = if m.blockflag != 0 {
            let prev = f.get_bits(1);
            let next = f.get_bits(1);
            (self.s.blocksize[1], prev, next)
        } else {
            (self.s.blocksize[0], 0, 0)
        };
        let blocksize_0 = self.s.blocksize[0];
        // WINDOWING
        let window_center = n >> 1;
        let (left_start, left_end) = if m.blockflag != 0 && prev == 0 {
            ((n - blocksize_0) >> 2, (n + blocksize_0) >> 2)
        } else {
            (0, window_center)
        };
        let (right_start, right_end) = if m.blockflag != 0 && next == 0 {
            ((n * 3 - blocksize_0) >> 2, (n * 3 + blocksize_0) >> 2)
        } else {
            (window_center, n)
        };
        Some(Initial {
            left_start,
            left_end,
            right_start,
            right_end,
            mode: i as usize,
        })
    }

    /// `vorbis_decode_packet_rest`: the rest of the packet, into the channel
    /// buffers. Returns the frame's length (`*len`); `left_start` may move
    /// forward to discard samples. `None` where the C would read or write
    /// outside its tables (damaged packets only).
    fn decode_packet_rest(&mut self, m: Mode, w: &mut Initial) -> Option<i32> {
        let s = &self.s;
        let stride = s.blocksize[1] as usize;
        let channels = s.channels as usize;
        let longest = s.longest_floorlist;
        let n = s.blocksize[usize::from(m.blockflag)];
        let map = &s.mappings[usize::from(m.mapping)];
        let n2 = n >> 1;

        // FLOORS
        let mut zero_channel = [false; 16];
        for (i, zero) in zero_channel.iter_mut().enumerate().take(channels) {
            let g = s
                .floors
                .get(map.submap_floor(usize::from(map.chan[i].mux)))?;
            let final_y = &mut self.final_y[i * longest..][..longest];
            *zero = !decode_floor1(&mut self.r, g, &s.codebooks, final_y);
        }
        // re-enable coupled channels if necessary
        let really_zero_channel = zero_channel;
        for chan in &map.chan[..usize::from(map.coupling_steps)] {
            let (magnitude, angle) = (usize::from(chan.magnitude), usize::from(chan.angle));
            if !zero_channel[magnitude] || !zero_channel[angle] {
                zero_channel[magnitude] = false;
                zero_channel[angle] = false;
            }
        }

        // RESIDUE DECODE
        for i in 0..map.submaps {
            let mut residue_buffers = [None; 16];
            let mut ch = 0;
            for (j, chan) in map.chan.iter().enumerate() {
                if chan.mux == i {
                    residue_buffers[ch] = (!zero_channel[j]).then_some(j);
                    ch += 1;
                }
            }
            // a 16th submap's floor overwrites the first residue number
            let residue = s.residues.get(map.submap_residue(usize::from(i)))?;
            let mut out = Buffers {
                data: &mut self.channel_buffers,
                stride,
                channels: &residue_buffers[..ch],
            };
            decode_residue(
                &mut self.r,
                &s.codebooks,
                residue,
                &mut out,
                n2,
                &mut self.part_classdata,
            );
        }

        // INVERSE COUPLING
        let half = n2 as usize;
        for chan in map.chan[..usize::from(map.coupling_steps)].iter().rev() {
            let (m, a) = two_channels(
                &mut self.channel_buffers,
                stride,
                usize::from(chan.magnitude),
                usize::from(chan.angle),
            );
            for (m, a) in m[..half].iter_mut().zip(&mut a[..half]) {
                let (mj, aj) = (*m, *a);
                let (m2, a2) = if mj > 0.0 {
                    if aj > 0.0 {
                        (mj, mj - aj)
                    } else {
                        (mj + aj, mj)
                    }
                } else if aj > 0.0 {
                    (mj, mj + aj)
                } else {
                    (mj - aj, mj)
                };
                *m = m2;
                *a = a2;
            }
        }

        // finish decoding the floors
        for (i, &zero) in really_zero_channel.iter().enumerate().take(channels) {
            let buffer = &mut self.channel_buffers[i * stride..][..stride];
            if zero {
                buffer[..half].fill(0.0);
            } else {
                let g = s
                    .floors
                    .get(map.submap_floor(usize::from(map.chan[i].mux)))?;
                if !do_floor(g, n, buffer, &self.final_y[i * longest..][..longest]) {
                    return None;
                }
            }
        }

        // INVERSE MDCT
        let tables = &s.tables[usize::from(m.blockflag)];
        for i in 0..channels {
            let buffer = &mut self.channel_buffers[i * stride..][..stride];
            inverse_mdct(buffer, n as usize, tables, &mut self.buf2);
        }

        // this shouldn't be necessary, unless we exited on an error
        // and want to flush to get to the next packet
        self.r.flush_packet();
        self.update_position(n, w)
    }

    /// The end of `vorbis_decode_packet_rest`: discarding at the start, and
    /// the sample position from the page's granule position.
    fn update_position(&mut self, n: i32, w: &mut Initial) -> Option<i32> {
        let n2 = n >> 1;
        let (right_start, right_end) = (w.right_start, w.right_end);
        if self.r.first_decode {
            // assume we start so first non-discarded sample is sample 0
            // (an intentional unsigned wrap-around)
            self.current_loc = 0_u32.wrapping_sub(n2 as u32);
            // we might have to discard samples "from" the next frame too
            self.discard_samples_deferred = n - right_end;
            self.current_loc_valid = true;
            self.r.first_decode = false;
        } else if self.discard_samples_deferred != 0 {
            if self.discard_samples_deferred >= right_start - w.left_start {
                self.discard_samples_deferred -= right_start - w.left_start;
                w.left_start = right_start;
            } else {
                w.left_start += self.discard_samples_deferred;
                self.discard_samples_deferred = 0;
            }
        }
        let left_start = w.left_start;
        // check if we have ogg information about the sample # for this packet
        if self.r.last_seg_which == self.r.end_seg_with_known_loc {
            // if we have a valid current loc, and this is final:
            if self.current_loc_valid && self.r.page_flag & PAGEFLAG_LAST_PAGE != 0 {
                let current_end = self.r.known_loc_for_packet;
                // then let's infer the size of the (probably) short final frame
                let frame_end = self
                    .current_loc
                    .wrapping_add((right_end - left_start) as u32);
                if current_end < frame_end {
                    let mut len = if current_end < self.current_loc {
                        // negative truncation, that's impossible!
                        0
                    } else {
                        current_end.wrapping_sub(self.current_loc) as i32
                    };
                    // this doesn't seem right, but has no ill effect on my test files
                    len = len.wrapping_add(left_start);
                    if len > right_end {
                        // this should never happen
                        len = right_end;
                    }
                    self.current_loc = self.current_loc.wrapping_add(len as u32);
                    return Some(len);
                }
            }
            // set current_loc to the position of left_start
            self.current_loc = self
                .r
                .known_loc_for_packet
                .wrapping_sub((n2 - left_start) as u32);
            self.current_loc_valid = true;
        }
        if self.current_loc_valid {
            self.current_loc = self
                .current_loc
                .wrapping_add((right_start - left_start) as u32);
        }
        // ignore samples after the window goes to 0
        Some(right_end)
    }

    /// `vorbis_decode_packet`: the frame's length, `left` and `right`.
    pub fn decode_packet(&mut self) -> Option<(i32, i32, i32)> {
        let mut w = self.decode_initial()?;
        let mode = self.s.modes[w.mode];
        let len = self.decode_packet_rest(mode, &mut w)?;
        Some((len, w.left_start, w.right_start))
    }
}
