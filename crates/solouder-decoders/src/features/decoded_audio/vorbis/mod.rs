// Ported from SoLoud src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa) and stb_vorbis 1.22 by Sean Barrett
// (public domain or MIT); altered for SoLouder.

//! Ogg Vorbis files to [`Pcm`] (`Wav::loadogg` with stb_vorbis), and the
//! Ogg Vorbis reader that streams share: a port of the pull-mode decoding
//! path of stb_vorbis 1.22 as SoLoud builds it (default options), from
//! memory and through SoLoud's `File`, bit-exact with it.

mod codebook;
mod decoder;
mod floor;
mod header;
mod huffman;
mod imdct;
mod layout;
mod length;
mod ogg;
mod packet;
mod residue;
mod seek;
mod setup;
mod source;
mod tables;

use std::io::{Read, Seek};

use solouder_core::{MAX_CHANNELS, Pcm};

use self::decoder::Vorbis;
use self::ogg::VorbisError;
pub(crate) use self::source::FileKind;
use self::source::{FileSource, Memory, SoloudFile};
use super::{DecodeError, DecodeLimits};

/// Why stb_vorbis refused to open a file.
fn open_error(e: VorbisError) -> DecodeError {
    let what = match e {
        VorbisError::NotOgg => return DecodeError::NotRecognized,
        VorbisError::TooManyChannels => {
            return DecodeError::Unsupported("Ogg Vorbis with more than 16 channels".to_owned());
        }
        VorbisError::FeatureNotSupported => {
            return DecodeError::Unsupported("Vorbis floor type 0".to_owned());
        }
        VorbisError::OggSkeletonNotSupported => {
            return DecodeError::Unsupported("Ogg skeleton streams".to_owned());
        }
        VorbisError::SetupTooLarge => {
            return DecodeError::Unsupported(
                "a Vorbis setup header needing more than 256 MiB".to_owned(),
            );
        }
        VorbisError::UnexpectedEof => "Ogg Vorbis headers cut short",
        VorbisError::InvalidFirstPage => "invalid Vorbis identification header",
        VorbisError::OutOfMem => "Vorbis header with an impossible size",
        VorbisError::MissingCapturePattern => "Ogg page without its capture pattern",
        VorbisError::InvalidStreamStructureVersion => "unknown Ogg stream structure version",
        VorbisError::ContinuedPacketFlagInvalid => "Ogg page continuation out of place",
        VorbisError::InvalidSetup | VorbisError::InvalidStream | VorbisError::None => {
            "invalid Vorbis setup header"
        }
    };
    DecodeError::Malformed(what.to_owned())
}

/// An Ogg Vorbis stream read through SoLoud's `File` over `R`, as
/// `WavStreamInstance` reads one with stb_vorbis (`stb_vorbis_open_file`
/// and the functions below). Frames are decoded one at a time into buffers
/// that the next call reuses.
pub(crate) struct VorbisFile<R> {
    v: Vorbis<FileSource<R>>,
    /// Where the last frame starts in each channel buffer, and its length.
    frame: (usize, usize),
}

impl<R: Read + Seek> VorbisFile<R> {
    /// Opens the stream that starts at `source`'s current position, read
    /// through a SoLoud `File` of `kind`: a `DiskFile` (`WavStream::load`) or
    /// a `MemoryFile` (`loadMem`, `loadToMem`), which differ at the end of the
    /// data. Reads the headers and the first frame, which only primes the
    /// overlap.
    pub(crate) fn open_as(source: R, kind: FileKind) -> Result<Self, DecodeError> {
        let file = SoloudFile::new(source, kind).map_err(DecodeError::Io)?;
        let (src, stream_len) = FileSource::open(file);
        let v = Vorbis::open(src, stream_len).map_err(open_error)?;
        Ok(Self { v, frame: (0, 0) })
    }

    /// The stream's channels, 1 to 16.
    pub(crate) fn channels(&self) -> u32 {
        self.v.s.channels as u32
    }

    /// The stream's sample rate, never 0.
    pub(crate) fn sample_rate(&self) -> u32 {
        self.v.s.sample_rate
    }

    /// `stb_vorbis_stream_length_in_samples`: frames up to the last page's
    /// granule position, found once by scanning the end of the stream; 0 if
    /// unknown. The read position is kept.
    pub(crate) fn length_in_samples(&mut self) -> u32 {
        self.v.stream_length_in_samples()
    }

    /// `stb_vorbis_get_frame_float`: decodes the next frame and returns its
    /// length in samples, which [`Self::output`] then holds. 0 at the end of
    /// the data and when a packet cannot be decoded; a frame can also be
    /// empty in the middle of the stream (SoLoud stops there too).
    pub(crate) fn get_frame_float(&mut self) -> usize {
        self.frame = self.v.get_frame_float();
        self.frame.1
    }

    /// Channel `channel` of the last decoded frame (stb's `outputs`); empty
    /// after a seek or past the stream's channels.
    pub(crate) fn output(&self, channel: usize) -> &[f32] {
        if channel >= self.v.s.channels as usize {
            return &[];
        }
        let (start, len) = self.frame;
        &self.v.channel_buffers[channel * self.v.stride() + start..][..len]
    }

    /// `stb_vorbis_seek_start`: back to the first audio page, priming the
    /// overlap with its first frame again. False if that frame fails.
    pub(crate) fn seek_start(&mut self) -> bool {
        self.frame = (0, 0);
        self.v.seek_start()
    }

    /// `stb_vorbis_seek_frame`: positions the stream so that the next
    /// [`Self::get_frame_float`] returns the frame holding `sample`, which
    /// starts at [`Self::sample_offset`]. False if the sample cannot be
    /// found (past the end, or the length is unknown).
    pub(crate) fn seek_frame(&mut self, sample: u32) -> bool {
        self.frame = (0, 0);
        self.v.seek_frame(sample)
    }

    /// `stb_vorbis_get_sample_offset`: the sample the next frame starts at;
    /// `None` where stb returns a negative number (no known position).
    pub(crate) fn sample_offset(&self) -> Option<u32> {
        u32::try_from(self.v.sample_offset()).ok()
    }
}

/// Decodes a whole Ogg Vorbis file held in `bytes` (SoLoud's `Wav` with
/// stb_vorbis).
///
/// The length is the last page's granule position, which must be found;
/// frames are decoded until stb_vorbis returns none (at the end, at a
/// packet it cannot decode, or at an empty frame), never past that length,
/// and the rest stays silent. Channels past [`MAX_CHANNELS`] are dropped,
/// as SoLoud drops them. The one allocation follows the length, within
/// `limits`. No input makes this panic.
pub fn decode_ogg(bytes: &[u8], limits: &DecodeLimits) -> Result<Pcm, DecodeError> {
    // stb_vorbis_open_memory takes an int length
    let len = i32::try_from(bytes.len())
        .map_err(|_| DecodeError::Unsupported("Ogg Vorbis files of 2 GiB or more".to_owned()))?;
    let mut v = Vorbis::open(Memory::new(bytes), len as u32).map_err(open_error)?;
    let sample_rate = v.s.sample_rate;
    // 0 means the length could not be determined (upstream #317).
    let frames = v.stream_length_in_samples();
    let channels = (v.s.channels as u32).min(MAX_CHANNELS);
    if frames == 0 {
        return Err(DecodeError::Malformed(
            "Ogg Vorbis whose length cannot be found".to_owned(),
        ));
    }
    let samples = u64::from(frames) * u64::from(channels);
    if samples > limits.max_samples || samples > u64::from(u32::MAX) {
        return Err(DecodeError::TooLarge {
            samples,
            limit: limits.max_samples.min(u64::from(u32::MAX)),
        });
    }
    let total = frames as usize;
    let len = usize::try_from(samples).map_err(|_| DecodeError::OutOfMemory)?;
    let mut data = Vec::new();
    data.try_reserve_exact(len)
        .map_err(|_| DecodeError::OutOfMemory)?;
    data.resize(len, 0.0);
    let stride = v.stride();
    let mut filled = 0;
    loop {
        let (start, n) = v.get_frame_float();
        // Never write past the length reported by the header.
        let n = n.min(total - filled);
        if n == 0 {
            break;
        }
        for c in 0..channels as usize {
            let from = &v.channel_buffers[c * stride + start..][..n];
            data[c * total + filled..][..n].copy_from_slice(from);
        }
        filled += n;
    }
    Ok(Pcm::from_planar(data, channels, sample_rate as f32)?)
}

#[cfg(test)]
#[path = "fuzz_tests.rs"]
mod fuzz_tests;
#[cfg(test)]
#[path = "table_tests.rs"]
mod table_tests;
#[cfg(test)]
#[path = "unit_tests.rs"]
mod unit_tests;
