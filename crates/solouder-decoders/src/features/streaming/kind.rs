// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Which decoder a stream uses (`WavStream::parse`), and opening it again
//! for each voice (`WavStreamInstance`'s constructor).

use std::io::{Read, Seek, SeekFrom};

use solouder_core::{MAX_CHANNELS, StreamFormat, StreamReader};

use super::flac_reader::FlacReader;
use super::mp3_reader::Mp3Reader;
use super::ogg_reader::OggReader;
use super::wav_reader::WavReader;
use crate::DecodeError;
use crate::features::decoded_audio::flac::FlacFile;
use crate::features::decoded_audio::mp3::Mp3File;
use crate::features::decoded_audio::riff::{self, Layout};
use crate::features::decoded_audio::vorbis::{FileKind, VorbisFile};

/// What a voice reads a stream from: the file it opened, or the bytes.
pub(crate) type BoxedSource = Box<dyn super::StreamInput>;

#[derive(Clone, Copy, Debug)]
pub(super) enum Kind {
    Wav(Layout),
    Flac,
    Ogg,
    Mp3,
}

/// A file's first four bytes: `OggS`, `RIFF` and `fLaC` name their
/// decoder; anything else is tried as MPEG audio.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Ogg,
    Riff,
    Flac,
    Other,
}

fn tagged(tag: &[u8]) -> Tag {
    match tag {
        b"OggS" => Tag::Ogg,
        b"RIFF" => Tag::Riff,
        b"fLaC" => Tag::Flac,
        _ => Tag::Other,
    }
}

/// Reads the first four bytes of `input` and goes back to its start.
fn read_tag(input: &mut BoxedSource) -> std::io::Result<Tag> {
    let mut tag = [0; 4];
    let mut got = 0;
    while got < tag.len() {
        match input.read(&mut tag[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    input.seek(SeekFrom::Start(0))?;
    Ok(tagged(&tag[..got]))
}

fn length(input: &mut BoxedSource) -> std::io::Result<u64> {
    let len = input.seek(SeekFrom::End(0))?;
    input.seek(SeekFrom::Start(0))?;
    Ok(len)
}

/// SoLoud keeps the frame count in 32 bits (upstream #401).
fn frame_count(frames: u64) -> Result<u32, DecodeError> {
    u32::try_from(frames)
        .map_err(|_| DecodeError::Unsupported("more than 2^32 - 1 frames".to_owned()))
}

/// `validStreamLayout`: 1 to [`MAX_CHANNELS`] channels and a sample rate.
fn format(channels: u32, sample_rate: u32, frames: u64) -> Result<StreamFormat, DecodeError> {
    if !(1..=MAX_CHANNELS).contains(&channels) {
        return Err(DecodeError::UnsupportedChannelCount(channels));
    }
    if sample_rate == 0 {
        return Err(DecodeError::InvalidSampleRate);
    }
    Ok(StreamFormat {
        channels,
        sample_rate: sample_rate as f32,
        frames: frame_count(frames)?,
    })
}

/// Reads the header of `input` and returns its decoder and format
/// (`WavStream::parse`). `file` is the SoLoud `File` the reference reads it
/// through, which stb_vorbis can tell apart at the end of the data.
pub(super) fn probe(
    mut input: BoxedSource,
    file: FileKind,
) -> Result<(Kind, StreamFormat), DecodeError> {
    match read_tag(&mut input).map_err(DecodeError::Io)? {
        Tag::Riff => {
            let len = length(&mut input).map_err(DecodeError::Io)?;
            let layout = riff::locate(&mut input, len)?;
            let format = format(layout.channels, layout.sample_rate, layout.frames())?;
            Ok((Kind::Wav(layout), format))
        }
        Tag::Flac => {
            let file = FlacFile::open(input)?;
            Ok((
                Kind::Flac,
                format(file.channels, file.sample_rate, file.frames)?,
            ))
        }
        Tag::Ogg => {
            let mut file = VorbisFile::open_as(input, file)?;
            // Extra channels are dropped, as in `WavStream::loadogg`, and an
            // unknown length (0) fails.
            let channels = file.channels().min(MAX_CHANNELS);
            let frames = file.length_in_samples();
            if frames == 0 {
                return Err(DecodeError::Unsupported(
                    "Ogg Vorbis whose length cannot be found".to_owned(),
                ));
            }
            Ok((
                Kind::Ogg,
                format(channels, file.sample_rate(), u64::from(frames))?,
            ))
        }
        Tag::Other => {
            let mut file = Mp3File::open(input)?;
            let frames = file.frame_count();
            Ok((
                Kind::Mp3,
                format(file.channels(), file.sample_rate(), frames)?,
            ))
        }
    }
}

/// A voice's reader of `input`, which must still hold a stream of `kind`
/// in `expected`'s format; `None` if it does not (the file was replaced or
/// is gone), which ends the voice at once. `file` is as for [`probe`].
pub(super) fn open(
    kind: Kind,
    expected: StreamFormat,
    mut input: BoxedSource,
    file: FileKind,
) -> Option<Box<dyn StreamReader>> {
    let frames = u64::from(expected.frames);
    let same = |channels: u32, rate: u32| {
        (channels, rate as f32) == (expected.channels, expected.sample_rate)
    };
    match (kind, read_tag(&mut input).ok()?) {
        (Kind::Wav(expected_layout), Tag::Riff) => {
            let len = length(&mut input).ok()?;
            let layout = riff::locate(&mut input, len).ok()?;
            let same = (layout.encoding, layout.channels, layout.sample_rate)
                == (
                    expected_layout.encoding,
                    expected_layout.channels,
                    expected_layout.sample_rate,
                );
            if !same {
                return None;
            }
            let reader = WavReader::new(input, layout, frames.min(layout.frames()))?;
            Some(Box::new(reader))
        }
        (Kind::Flac, Tag::Flac) => {
            let file = FlacFile::open(input).ok()?;
            same(file.channels, file.sample_rate).then(|| Box::new(FlacReader::new(file)) as _)
        }
        (Kind::Ogg, Tag::Ogg) => {
            let file = VorbisFile::open_as(input, file).ok()?;
            let channels = file.channels().min(MAX_CHANNELS);
            same(channels, file.sample_rate())
                .then(|| Box::new(OggReader::new(file, channels, frames)) as _)
        }
        (Kind::Mp3, Tag::Other) => {
            let file = Mp3File::open(input).ok()?;
            same(file.channels(), file.sample_rate()).then(|| Box::new(Mp3Reader::new(file)) as _)
        }
        _ => None,
    }
}
