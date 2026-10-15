// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! `struct stb_vorbis` and its pull-mode entry points: opening
//! (`stb_vorbis_open_memory`, `stb_vorbis_open_file`), `vorbis_finish_frame`,
//! `vorbis_pump_first_frame` and `stb_vorbis_get_frame_float`.

use super::header::start_decoder;
use super::ogg::{ProbedPage, Reader, VorbisError};
use super::setup::Setup;
use super::source::Source;

/// One Vorbis stream being decoded, as `struct stb_vorbis` in pull mode.
pub(super) struct Vorbis<S> {
    pub r: Reader<S>,
    pub s: Setup,
    /// `channel_buffers`: `blocksize_1` values per channel.
    pub channel_buffers: Vec<f32>,
    /// `previous_window`: `blocksize_1 / 2` values per channel.
    pub previous_window: Vec<f32>,
    /// `finalY`: `longest_floorlist` values per channel.
    pub final_y: Vec<i16>,
    pub previous_length: i32,
    pub current_loc: u32,
    pub current_loc_valid: bool,
    pub discard_samples_deferred: i32,
    pub samples_output: u32,
    pub channel_buffer_start: i32,
    pub channel_buffer_end: i32,
    pub first_audio_page_offset: u32,
    pub p_last: ProbedPage,
    pub total_samples: u32,
    /// The inverse MDCT's `buf2` (stb's `temp_alloc`).
    pub buf2: Vec<f32>,
    /// `decode_residue`'s `part_classdata` (stb's `temp_block_array`).
    pub part_classdata: Vec<usize>,
}

impl<S: Source> Vorbis<S> {
    /// `start_decoder` and the first frame, which only fills the window
    /// (`stb_vorbis_open_memory` and `stb_vorbis_open_file_section`).
    pub fn open(src: S, stream_len: u32) -> Result<Self, VorbisError> {
        let mut r = Reader::new(src, stream_len);
        let (s, first_audio_page_offset) = start_decoder(&mut r)?;
        let channels = s.channels as usize;
        let blocksize_1 = s.blocksize[1] as usize;
        let longest = s.longest_floorlist;
        let mut f = Self {
            r,
            channel_buffers: vec![0.0; channels * blocksize_1],
            previous_window: vec![0.0; channels * blocksize_1 / 2],
            final_y: vec![0; channels * longest],
            s,
            previous_length: 0,
            current_loc: 0,
            current_loc_valid: false,
            discard_samples_deferred: 0,
            samples_output: 0,
            channel_buffer_start: 0,
            channel_buffer_end: 0,
            first_audio_page_offset,
            p_last: ProbedPage::default(),
            total_samples: 0,
            buf2: vec![0.0; blocksize_1 / 2],
            part_classdata: Vec::new(),
        };
        f.pump_first_frame();
        Ok(f)
    }

    /// `blocksize_1`, the stride of `channel_buffers`.
    pub fn stride(&self) -> usize {
        self.s.blocksize[1] as usize
    }

    /// `get_window`: which blocksize's window has an overlap of `len`.
    fn window(&self, len: i32) -> Option<usize> {
        let len = len.wrapping_shl(1);
        if len == self.s.blocksize[0] {
            Some(0)
        } else if len == self.s.blocksize[1] {
            Some(1)
        } else {
            None
        }
    }

    /// `vorbis_finish_frame`: overlaps the frame with the previous one,
    /// keeps its end for the next, and returns how many samples it gives.
    pub fn finish_frame(&mut self, len: i32, left: i32, mut right: i32) -> i32 {
        let stride = self.stride();
        let half = stride / 2;
        let channels = self.s.channels as usize;
        // mixin from previous window
        if self.previous_length != 0 {
            let n = self.previous_length;
            let Some(w) = self.window(n) else {
                return 0;
            };
            let w = &self.s.tables[w].window;
            let n = n as usize;
            // a window is half a blocksize, so n is at most `half`
            let (start, end) = (left as usize, left as usize + n);
            if left < 0 || end > stride || w.len() < n {
                // damaged packets only: the C writes past its buffer
                return 0;
            }
            for i in 0..channels {
                let cb = &mut self.channel_buffers[i * stride..][start..end];
                let pw = &self.previous_window[i * half..][..n];
                for (j, (c, &p)) in cb.iter_mut().zip(pw).enumerate() {
                    *c = *c * w[j] + p * w[n - 1 - j];
                }
            }
        }
        let prev = self.previous_length;
        // last half of this data becomes previous window
        self.previous_length = len - right;
        if right >= 0 && len > right && (len as usize) <= stride && ((len - right) as usize) <= half
        {
            let (right, len) = (right as usize, len as usize);
            for i in 0..channels {
                let from = &self.channel_buffers[i * stride..][right..len];
                self.previous_window[i * half..][..len - right].copy_from_slice(from);
            }
        }
        if prev == 0 {
            // there was no previous packet, so this data isn't valid...
            return 0;
        }
        // truncate a short frame
        if len < right {
            right = len;
        }
        self.samples_output = self.samples_output.wrapping_add((right - left) as u32);
        right - left
    }

    /// `vorbis_pump_first_frame`.
    pub fn pump_first_frame(&mut self) -> bool {
        match self.decode_packet() {
            Some((len, left, right)) => {
                self.finish_frame(len, left, right);
                true
            }
            None => false,
        }
    }

    /// `stb_vorbis_get_frame_float`: decodes the next frame. Returns where
    /// its samples start in each channel buffer and how many there are; 0
    /// samples at the end of the data or when a packet fails.
    pub fn get_frame_float(&mut self) -> (usize, usize) {
        let Some((len, left, right)) = self.decode_packet() else {
            self.channel_buffer_start = 0;
            self.channel_buffer_end = 0;
            return (0, 0);
        };
        let len = self.finish_frame(len, left, right);
        self.channel_buffer_start = left;
        self.channel_buffer_end = left + len;
        (left.max(0) as usize, len.max(0) as usize)
    }

    /// `stb_vorbis_get_sample_offset`.
    pub fn sample_offset(&self) -> i32 {
        if self.current_loc_valid {
            self.current_loc as i32
        } else {
            -1
        }
    }
}
