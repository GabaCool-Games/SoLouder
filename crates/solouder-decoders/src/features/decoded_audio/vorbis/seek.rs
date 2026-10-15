// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! Seeking: `get_seek_page_info`, `go_to_page_before`,
//! `seek_to_sample_coarse`, `peek_decode_initial`, `stb_vorbis_seek_frame`
//! and `stb_vorbis_seek_start`.

use super::decoder::Vorbis;
use super::length::OGG_PAGE_HEADER;
use super::ogg::{PAGEFLAG_CONTINUED_PACKET, ProbedPage};
use super::packet::Initial;
use super::source::Source;
use super::tables::ilog;

// Until the streaming reader drives `VorbisFile`, only tests use this.
#[cfg_attr(not(test), allow(dead_code))]
impl<S: Source> Vorbis<S> {
    /// `get_seek_page_info`: the page at the current position, which is
    /// kept. `None` if there is no page there, also when its header or
    /// lacing values cannot be read (the C goes on with whatever its
    /// uninitialised arrays hold).
    fn get_seek_page_info(&mut self) -> Option<ProbedPage> {
        let f = &mut self.r;
        let page_start = f.file_offset();
        let mut header = [0_u8; 27];
        if !f.getn(&mut header) || header[..4] != OGG_PAGE_HEADER {
            return None;
        }
        let mut lacing = [0_u8; 255];
        let count = usize::from(header[26]);
        if !f.getn(&mut lacing[..count]) {
            return None;
        }
        let len: u32 = lacing[..count].iter().map(|&l| u32::from(l)).sum();
        let page = ProbedPage {
            page_start,
            page_end: page_start.wrapping_add(27 + count as u32 + len),
            last_decoded_sample: u32::from_le_bytes([header[6], header[7], header[8], header[9]]),
        };
        // restore file state to where we were
        f.set_file_offset(page_start);
        Some(page)
    }

    /// `go_to_page_before`: moves to the page that holds `limit_offset`.
    fn go_to_page_before(&mut self, limit_offset: u32) -> bool {
        let previous_safe =
            if limit_offset >= 65536 && limit_offset - 65536 >= self.first_audio_page_offset {
                limit_offset - 65536
            } else {
                self.first_audio_page_offset
            };
        self.r.set_file_offset(previous_safe);
        while let Some((end, _)) = self.find_page() {
            if end >= limit_offset && self.r.file_offset() < limit_offset {
                return true;
            }
            self.r.set_file_offset(end);
        }
        false
    }

    /// `peek_decode_initial`: `vorbis_decode_initial` without advancing.
    fn peek_decode_initial(&mut self) -> Option<Initial> {
        let w = self.decode_initial()?;
        // either 1 or 2 bytes were read, figure out which so we can rewind
        let mut bits_read = 1 + ilog(self.s.modes.len() as i32 - 1);
        if self.s.modes[w.mode].blockflag != 0 {
            bits_read += 2;
        }
        let bytes_read = (bits_read + 7) / 8;
        let f = &mut self.r;
        f.bytes_in_seg = f.bytes_in_seg.wrapping_add(bytes_read as u8);
        f.packet_bytes -= bytes_read;
        f.skip(-bytes_read);
        if f.next_seg == -1 {
            f.next_seg = f.segment_count - 1;
        } else {
            f.next_seg -= 1;
        }
        f.valid_bits = 0;
        Some(w)
    }

    /// `seek_to_sample_coarse`: starts decoding at a page whose frames end at
    /// or before `sample_number`, found by bisection on granule positions.
    fn seek_to_sample_coarse(&mut self, sample_number: u32) -> bool {
        // find the last page and validate the target sample
        let stream_length = self.stream_length_in_samples();
        if stream_length == 0 || sample_number > stream_length {
            return false;
        }
        // the most the window center (the granule position) and the right
        // start can differ by
        let padding = ((self.s.blocksize[1] - self.s.blocksize[0]) >> 2) as u32;
        let last_sample_limit = sample_number.saturating_sub(padding);
        let mut left = self.r.p_first;
        while left.last_decoded_sample == !0 {
            // (untested) the first page does not have a 'last_decoded_sample'
            self.r.set_file_offset(left.page_end);
            let Some(page) = self.get_seek_page_info() else {
                return self.seek_failed();
            };
            left = page;
        }
        let mut right = self.p_last;
        // starting from the start is handled differently
        if last_sample_limit <= left.last_decoded_sample {
            return self.seek_start() && self.current_loc <= sample_number;
        }
        let (mut offset, mut bytes_per_sample) = (0.0_f64, 0.0_f64);
        let mut probe = 0;
        let mut mid = ProbedPage::default();
        while left.page_end != right.page_start {
            if left.page_end > right.page_start {
                // stb asserts against this; damaged files only
                return self.seek_failed();
            }
            // search range in bytes
            let delta = right.page_start - left.page_end;
            if delta <= 65536 {
                // there's only 64K left to search - handle it linearly
                self.r.set_file_offset(left.page_end);
            } else {
                if probe < 2 {
                    if probe == 0 {
                        // first probe (interpolate)
                        let data_bytes = f64::from(right.page_end.wrapping_sub(left.page_start));
                        bytes_per_sample = data_bytes / f64::from(right.last_decoded_sample);
                        let samples = last_sample_limit.wrapping_sub(left.last_decoded_sample);
                        offset = f64::from(left.page_start) + bytes_per_sample * f64::from(samples);
                    } else {
                        // second probe (try to bound the other side)
                        let mut error = (f64::from(last_sample_limit)
                            - f64::from(mid.last_decoded_sample))
                            * bytes_per_sample;
                        if (0.0..8000.0).contains(&error) {
                            error = 8000.0;
                        }
                        if error < 0.0 && error > -8000.0 {
                            error = -8000.0;
                        }
                        offset += error * 2.0;
                    }
                    // ensure the offset is valid
                    if offset < f64::from(left.page_end) {
                        offset = f64::from(left.page_end);
                    }
                    if offset > f64::from(right.page_start - 65536) {
                        offset = f64::from(right.page_start - 65536);
                    }
                    // in range here, or NaN, which MSVC converts to 0 too
                    self.r.set_file_offset(offset as u32);
                } else {
                    // binary search for large ranges (offset by 32K to
                    // ensure we don't hit the right page)
                    let at = left.page_end.wrapping_add(delta / 2).wrapping_sub(32768);
                    self.r.set_file_offset(at);
                }
                if self.find_page().is_none() {
                    return self.seek_failed();
                }
            }
            loop {
                let Some(page) = self.get_seek_page_info() else {
                    return self.seek_failed();
                };
                mid = page;
                if mid.last_decoded_sample != !0 {
                    break;
                }
                // (untested) no frames end on this page
                self.r.set_file_offset(mid.page_end);
            }
            // if we've just found the last page again then we're in a tricky
            // file, and we're close enough (if it wasn't an interpolation probe)
            if mid.page_start == right.page_start {
                if probe >= 2 || delta <= 65536 {
                    break;
                }
            } else if last_sample_limit < mid.last_decoded_sample {
                right = mid;
            } else {
                left = mid;
            }
            probe += 1;
        }
        self.start_at_page(left.page_start, sample_number)
    }

    /// The end of `seek_to_sample_coarse`: goes back to the start of the
    /// last packet that ends on the page at `page_start` and decodes it.
    fn start_at_page(&mut self, mut page_start: u32, sample_number: u32) -> bool {
        self.r.set_file_offset(page_start);
        if !self.r.start_page() {
            return false;
        }
        let mut end_pos = self.r.end_seg_with_known_loc;
        let start_seg_with_known_loc = loop {
            let mut i = end_pos;
            while i > 0 && self.r.segments[(i - 1) as usize] == 255 {
                i -= 1;
            }
            if i > 0 || self.r.page_flag & PAGEFLAG_CONTINUED_PACKET == 0 {
                break i;
            }
            // (untested) the final packet begins on an earlier page
            if !self.go_to_page_before(page_start) {
                return self.seek_failed();
            }
            page_start = self.r.file_offset();
            if !self.r.start_page() {
                return self.seek_failed();
            }
            end_pos = self.r.segment_count - 1;
        };
        // prepare to start decoding
        self.current_loc_valid = false;
        let f = &mut self.r;
        f.last_seg = false;
        f.valid_bits = 0;
        f.packet_bytes = 0;
        f.bytes_in_seg = 0;
        self.previous_length = 0;
        f.next_seg = start_seg_with_known_loc;
        for i in 0..start_seg_with_known_loc.max(0) as usize {
            let len = i32::from(f.segments[i]);
            f.skip(len);
        }
        // start decoding (optimizable - this frame is generally discarded)
        self.pump_first_frame() && self.current_loc <= sample_number
    }

    /// The `error:` exit of `seek_to_sample_coarse`.
    fn seek_failed(&mut self) -> bool {
        // try to restore the file to a valid state
        self.seek_start();
        false
    }

    /// `stb_vorbis_seek_frame`: positions the decoder so that the next frame
    /// holds `sample_number`.
    pub fn seek_frame(&mut self, sample_number: u32) -> bool {
        // fast page-level search
        if !self.seek_to_sample_coarse(sample_number) {
            return false;
        }
        // linear search for the relevant packet
        let blocksize = self.s.blocksize;
        let max_frame_samples = ((blocksize[1] * 3 - blocksize[0]) >> 2) as u32;
        while self.current_loc < sample_number {
            let Some(w) = self.peek_decode_initial() else {
                return false;
            };
            // calculate the number of samples returned by the next frame
            let frame_samples = (w.right_start - w.left_start) as u32;
            let next = self.current_loc.wrapping_add(frame_samples);
            if next > sample_number {
                // the next frame will contain the sample
                return true;
            } else if next.wrapping_add(max_frame_samples) > sample_number {
                // there's a chance the frame after this could contain the sample
                self.pump_first_frame();
            } else {
                // this frame is too early to be relevant
                self.current_loc = next;
                self.previous_length = 0;
                self.r.maybe_start_packet();
                self.r.flush_packet();
            }
        }
        // the next frame should start with the sample
        self.current_loc == sample_number
    }

    /// `stb_vorbis_seek_start`.
    pub fn seek_start(&mut self) -> bool {
        let offset = self.first_audio_page_offset;
        self.r.set_file_offset(offset);
        self.previous_length = 0;
        self.r.first_decode = true;
        self.r.next_seg = -1;
        self.pump_first_frame()
    }
}
