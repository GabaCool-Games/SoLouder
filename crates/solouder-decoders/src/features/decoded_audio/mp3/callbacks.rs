// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution) and SoLoud src/audiosource/wav/soloud_wavstream.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! dr_mp3's callback path (`drmp3_decode_next_frame_ex__callbacks`) over a
//! reader, with SoLoud's read and seek callbacks (`drmp3_read_func`,
//! `drmp3_seek_func`).
//!
//! The data is buffered as dr_mp3 buffers it: in chunks of 64 KiB, topped
//! up whenever less than 16 KiB is left. The frame search only sees what is
//! buffered, so this decides, for damaged files and long tags, which frames
//! are found; the buffering is kept as it is for that reason.

use std::io::{ErrorKind, Read, Seek, SeekFrom};

use super::decoder::{Decoder, FrameInfo, MAX_SAMPLES_PER_FRAME};
use super::drmp3::Source;
use super::header::frame_samples;

/// `DRMP3_MIN_DATA_CHUNK_SIZE`.
const MIN_DATA_CHUNK_SIZE: usize = 16384;
/// `DRMP3_DATA_CHUNK_SIZE`.
const DATA_CHUNK_SIZE: usize = MIN_DATA_CHUNK_SIZE * 4;

/// A reader and dr_mp3's buffer over it (`pData`, `dataCapacity` as the
/// buffer's length, `dataSize`, `dataConsumed`, `atEnd`).
pub(super) struct Callbacks<R> {
    reader: R,
    data: Vec<u8>,
    size: usize,
    consumed: usize,
    at_end: bool,
}

impl<R: Read + Seek> Callbacks<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            data: Vec::new(),
            size: 0,
            consumed: 0,
            at_end: false,
        }
    }

    /// `drmp3__on_read` with SoLoud's `File::read`: fills `data[from..]` as
    /// far as the reader goes. A read error ends the read, as a short read
    /// from SoLoud's files does.
    fn read_into(&mut self, from: usize) -> usize {
        let buf = &mut self.data[from..];
        let mut read = 0;
        while read < buf.len() {
            match self.reader.read(&mut buf[read..]) {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        read
    }

    /// `drmp3__realloc_from_callbacks`: false if the memory is not there.
    fn grow_to(&mut self, capacity: usize) -> bool {
        let more = capacity.saturating_sub(self.data.len());
        if self.data.try_reserve_exact(more).is_err() {
            return false;
        }
        self.data.resize(capacity, 0);
        true
    }

    /// Moves the unconsumed bytes to the start of the buffer.
    fn move_down(&mut self) {
        let (from, size) = (self.consumed, self.size);
        self.data.copy_within(from..from + size, 0);
        self.consumed = 0;
    }
}

impl<R: Read + Seek> Source for Callbacks<R> {
    fn decode_next(
        &mut self,
        decoder: &mut Decoder,
        mut pcm: Option<&mut [f32; MAX_SAMPLES_PER_FRAME]>,
    ) -> (u32, FrameInfo) {
        if self.at_end {
            return (0, FrameInfo::default());
        }
        loop {
            // minimp3 recommends at least 16 KiB of data per call.
            if self.size < MIN_DATA_CHUNK_SIZE {
                self.move_down();
                if self.data.len() < DATA_CHUNK_SIZE && !self.grow_to(DATA_CHUNK_SIZE) {
                    return (0, FrameInfo::default()); // out of memory
                }
                let read = self.read_into(self.size);
                if read == 0 && self.size == 0 {
                    self.at_end = true;
                    return (0, FrameInfo::default()); // no data
                }
                self.size += read;
            }
            if self.size > i32::MAX as usize {
                self.at_end = true;
                return (0, FrameInfo::default()); // file too big
            }

            let window = &self.data[self.consumed..self.consumed + self.size];
            let (frames, info) = decoder.decode_frame(window, pcm.as_deref_mut());
            let used = usize::try_from(info.frame_bytes)
                .unwrap_or(0)
                .min(self.size);
            self.consumed += used;
            self.size -= used;

            if frames > 0 {
                return (frame_samples(&decoder.header), info);
            } else if used == 0 {
                // Need more data (only reached with nothing buffered).
                self.move_down();
                if self.data.len() == self.size {
                    let capacity = self.data.len() + DATA_CHUNK_SIZE;
                    if !self.grow_to(capacity) {
                        return (0, FrameInfo::default()); // out of memory
                    }
                }
                let read = self.read_into(self.size);
                if read == 0 {
                    self.at_end = true;
                    return (0, FrameInfo::default()); // error reading more data
                }
                self.size += read;
            }
        }
    }

    /// SoLoud's `drmp3_seek_func` moves its file and reports success; here
    /// a reader that cannot seek reports failure.
    fn seek_start(&mut self) -> bool {
        self.reader.seek(SeekFrom::Start(0)).is_ok()
    }

    fn reset(&mut self) {
        self.size = 0;
        self.at_end = false;
    }
}
