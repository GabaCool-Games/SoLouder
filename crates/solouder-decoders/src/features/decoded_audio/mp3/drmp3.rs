// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution); altered for SoLouder.

//! dr_mp3's pull API over minimp3 (`drmp3`): the first frame decoded on
//! opening, PCM frames read across MP3 frames, the seek to the start and the
//! frame count, with `DR_MP3_FLOAT_OUTPUT`. The seek table API is not used
//! by SoLoud and is not ported.

use super::decoder::{Decoder, FrameInfo, MAX_SAMPLES_PER_FRAME};
use super::header::frame_samples;

/// Where the MPEG audio bytes come from: memory or read and seek callbacks.
pub(super) trait Source {
    /// `drmp3_decode_next_frame_ex__memory` or `__callbacks` up to the
    /// frame they decode: the PCM frames of the next frame that decodes
    /// (0 at the end of the data), and its description.
    fn decode_next(
        &mut self,
        decoder: &mut Decoder,
        pcm: Option<&mut [f32; MAX_SAMPLES_PER_FRAME]>,
    ) -> (u32, FrameInfo);

    /// `onSeek(0, drmp3_seek_origin_start)`.
    fn seek_start(&mut self) -> bool;

    /// The source's part of `drmp3_reset` (`dataSize = 0`, `atEnd =
    /// false`).
    fn reset(&mut self);
}

/// `drmp3_decode_next_frame_ex__memory`'s view of the whole file.
pub(super) struct Memory<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Memory<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl Source for Memory<'_> {
    fn decode_next(
        &mut self,
        decoder: &mut Decoder,
        mut pcm: Option<&mut [f32; MAX_SAMPLES_PER_FRAME]>,
    ) -> (u32, FrameInfo) {
        loop {
            let rest = self.data.get(self.pos..).unwrap_or_default();
            let (frames, info) = decoder.decode_frame(rest, pcm.as_deref_mut());
            let used = usize::try_from(info.frame_bytes).unwrap_or(0);
            if frames > 0 {
                self.pos += used;
                return (frame_samples(&decoder.header), info);
            } else if used > 0 {
                // No frames were read, but one was skipped.
                self.pos += used;
            } else {
                return (0, info);
            }
        }
    }

    fn seek_start(&mut self) -> bool {
        self.pos = 0;
        true
    }

    fn reset(&mut self) {}
}

/// `drmp3`: an open MPEG audio stream.
pub(super) struct Drmp3<S> {
    pub source: S,
    decoder: Box<Decoder>,
    /// The channels of the first frame, which every read returns.
    pub channels: u32,
    /// The sample rate of the first frame.
    pub sample_rate: u32,
    /// The channels of the frame in `pcm`.
    frame_channels: u32,
    consumed: u32,
    remaining: u32,
    /// `pcmFrames`: the frame decoded last, interleaved. Never cleared, so a
    /// frame with fewer channels than the first is read with what earlier
    /// frames left after it, as in C.
    pcm: Box<[f32; MAX_SAMPLES_PER_FRAME]>,
    current: u64,
}

impl<S: Source> Drmp3<S> {
    /// `drmp3_init` and `drmp3_init_memory`: decodes the first frame; `None`
    /// if there is none.
    pub fn init(source: S) -> Option<Self> {
        let mut mp3 = Self {
            source,
            decoder: Decoder::new(),
            channels: 0,
            sample_rate: 0,
            frame_channels: 0,
            consumed: 0,
            remaining: 0,
            pcm: Box::new([0.0; MAX_SAMPLES_PER_FRAME]),
            current: 0,
        };
        let (frames, info) = mp3.decode_next_frame(true);
        if frames == 0 {
            return None;
        }
        mp3.channels = info.channels;
        mp3.sample_rate = info.hz;
        Some(mp3)
    }

    /// `drmp3_decode_next_frame_ex`, into `pcm` or (counting) nowhere.
    fn decode_next_frame(&mut self, with_pcm: bool) -> (u32, FrameInfo) {
        let pcm = if with_pcm { Some(&mut *self.pcm) } else { None };
        let (frames, info) = self.source.decode_next(&mut self.decoder, pcm);
        if frames > 0 {
            self.consumed = 0;
            self.remaining = frames;
            self.frame_channels = info.channels;
        }
        (frames, info)
    }

    /// `drmp3_read_pcm_frames_f32` (`drmp3_read_pcm_frames_raw`): up to
    /// `frames` frames of `channels` samples into `out` (or skipped), fewer
    /// at the end of the data.
    pub fn read_pcm_frames(&mut self, mut out: Option<&mut [f32]>, mut frames: u64) -> u64 {
        let channels = self.channels as usize;
        let mut total = 0_u64;
        while frames > 0 {
            let n = u64::from(self.remaining).min(frames) as u32;
            if let Some(out) = out.as_deref_mut() {
                let len = n as usize * channels;
                let to = total as usize * channels;
                let from = self.consumed as usize * self.frame_channels as usize;
                if let (Some(dst), Some(src)) =
                    (out.get_mut(to..to + len), self.pcm.get(from..from + len))
                {
                    dst.copy_from_slice(src);
                }
            }
            self.current += u64::from(n);
            self.consumed += n;
            self.remaining -= n;
            total += u64::from(n);
            frames -= u64::from(n);
            if frames == 0 {
                break;
            }
            if self.decode_next_frame(true).0 == 0 {
                break;
            }
        }
        total
    }

    /// `drmp3_seek_to_start_of_stream` (with `drmp3_reset`).
    fn seek_to_start(&mut self) -> bool {
        if !self.source.seek_start() {
            return false;
        }
        self.consumed = 0;
        self.remaining = 0;
        self.current = 0;
        self.source.reset();
        self.decoder.init();
        true
    }

    /// `drmp3_seek_to_pcm_frame` without a seek table: back to the start,
    /// and forward by decoding.
    pub fn seek_to_pcm_frame(&mut self, frame: u64) -> bool {
        if frame == 0 {
            return self.seek_to_start();
        }
        if frame == self.current {
            return true;
        }
        if frame < self.current && !self.seek_to_start() {
            return false;
        }
        let offset = frame - self.current;
        self.read_pcm_frames(None, offset) == offset
    }

    /// `drmp3_get_pcm_frame_count`: checks every frame from the start
    /// (layer 3 frames whose bit reservoir is short count as none, layer 1
    /// and 2 frames are not decoded), then goes back to the frame it was
    /// on. 0 if the source cannot seek.
    pub fn pcm_frame_count(&mut self) -> u64 {
        let current = self.current;
        if !self.seek_to_start() {
            return 0;
        }
        let mut total = 0_u64;
        loop {
            let (frames, _) = self.decode_next_frame(false);
            if frames == 0 {
                break;
            }
            total += u64::from(frames);
        }
        if !self.seek_to_start() || !self.seek_to_pcm_frame(current) {
            return 0;
        }
        total
    }
}
