//! The RIFF/WAVE container: finds the format and data chunks and validates
//! the format before any sample is read.
//!
//! Written from the RIFF and WAVEFORMATEXTENSIBLE layouts, following the
//! rules of dr_wav (SoLoud's WAV reader) where files are ambiguous: chunks
//! are padded to even sizes, the RIFF size field is not trusted, an
//! extensible header's format is the first two bytes of its sub-format GUID,
//! and the channel mask is ignored.

use std::io::{Cursor, Read, Seek, SeekFrom};

use solouder_core::MAX_CHANNELS;

use super::DecodeError;

/// How one sample is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    /// Unsigned 8-bit integer.
    U8,
    /// Signed 16-bit integer.
    S16,
    /// Signed 24-bit integer in three bytes.
    S24,
    /// Signed 32-bit integer.
    S32,
    /// IEEE 754 single precision.
    F32,
    /// IEEE 754 double precision.
    F64,
    /// G.711 A-law.
    ALaw,
    /// G.711 mu-law.
    MuLaw,
}

impl Encoding {
    /// Bytes per sample.
    pub fn width(self) -> usize {
        match self {
            Self::U8 | Self::ALaw | Self::MuLaw => 1,
            Self::S16 => 2,
            Self::S24 => 3,
            Self::S32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }
}

/// A validated WAV file.
#[derive(Debug)]
pub(crate) struct Wav<'a> {
    pub encoding: Encoding,
    /// 1 to `MAX_CHANNELS`.
    pub channels: u32,
    /// Nonzero.
    pub sample_rate: u32,
    /// The part of the data chunk present in the file; it may end inside a
    /// frame.
    pub data: &'a [u8],
}

const PCM: u16 = 0x0001;
const IEEE_FLOAT: u16 = 0x0003;
const ALAW: u16 = 0x0006;
const MULAW: u16 = 0x0007;
const EXTENSIBLE: u16 = 0xFFFE;

/// The base `WAVEFORMAT` fields plus `wBitsPerSample`.
const FORMAT_LEN: usize = 16;
/// `WAVEFORMATEXTENSIBLE`: the base, `cbSize`, valid bits, channel mask and
/// the sub-format GUID.
const EXTENSIBLE_LEN: usize = 40;
const SUB_FORMAT_AT: usize = 24;

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn malformed(what: &str) -> DecodeError {
    DecodeError::Malformed(what.to_owned())
}

/// Where a WAV file's samples are and how they are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub encoding: Encoding,
    /// 1 to `MAX_CHANNELS`.
    pub channels: u32,
    /// Nonzero.
    pub sample_rate: u32,
    /// Byte offset of the first sample.
    pub data_start: u64,
    /// Bytes of the data chunk present in the file; it may end inside a
    /// frame.
    pub data_len: u64,
}

impl Layout {
    /// Bytes per frame.
    pub fn frame_bytes(&self) -> usize {
        self.channels as usize * self.encoding.width()
    }

    /// Whole frames present.
    pub fn frames(&self) -> u64 {
        self.data_len / self.frame_bytes() as u64
    }
}

/// Walks the chunks of `bytes` up to the data chunk.
pub(crate) fn parse(bytes: &[u8]) -> Result<Wav<'_>, DecodeError> {
    let layout = locate(&mut Cursor::new(bytes), bytes.len() as u64)?;
    // Both fit in `bytes`, so they fit a usize.
    let start = layout.data_start as usize;
    Ok(Wav {
        encoding: layout.encoding,
        channels: layout.channels,
        sample_rate: layout.sample_rate,
        data: &bytes[start..start + layout.data_len as usize],
    })
}

/// Walks the chunks of a `len`-byte file up to the data chunk, reading
/// only chunk headers and the format chunk.
pub(crate) fn locate<R: Read + Seek>(input: &mut R, len: u64) -> Result<Layout, DecodeError> {
    let mut riff = [0_u8; 12];
    if read_full(input, &mut riff)? < 12 || &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
        return Err(DecodeError::NotRecognized);
    }
    let mut format = None;
    let mut at = 12_u64;
    loop {
        let mut header = [0_u8; 8];
        input.seek(SeekFrom::Start(at)).map_err(DecodeError::Io)?;
        if at >= len || read_full(input, &mut header)? < 8 {
            break;
        }
        let size = u64::from(u32_at(&header, 4));
        let body = at + 8;
        let available = len.saturating_sub(body);
        match &header[..4] {
            b"fmt " => {
                if size > available {
                    return Err(malformed("the format chunk is cut off"));
                }
                // Only the first EXTENSIBLE_LEN bytes are ever read.
                let mut chunk = [0_u8; EXTENSIBLE_LEN];
                let want = size.min(EXTENSIBLE_LEN as u64) as usize;
                if read_full(input, &mut chunk[..want])? < want {
                    return Err(malformed("the format chunk is cut off"));
                }
                format = Some(parse_format(&chunk[..want])?);
            }
            b"data" => {
                let (encoding, channels, sample_rate) =
                    format.ok_or_else(|| malformed("data chunk before the format chunk"))?;
                return Ok(Layout {
                    encoding,
                    channels,
                    sample_rate,
                    data_start: body,
                    data_len: size.min(available),
                });
            }
            _ => {}
        }
        // Chunk bodies are padded to an even length.
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    Err(malformed(if format.is_some() {
        "no data chunk"
    } else {
        "no format chunk"
    }))
}

/// Reads until `buf` is full or the input ends; returns the bytes read.
fn read_full<R: Read>(input: &mut R, buf: &mut [u8]) -> Result<usize, DecodeError> {
    let mut filled = 0;
    while filled < buf.len() {
        match input.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(DecodeError::Io(e)),
        }
    }
    Ok(filled)
}

/// Reads a `fmt ` chunk into its encoding, channel count and sample rate.
fn parse_format(chunk: &[u8]) -> Result<(Encoding, u32, u32), DecodeError> {
    if chunk.len() < FORMAT_LEN {
        return Err(malformed("the format chunk is shorter than 16 bytes"));
    }
    let mut tag = u16_at(chunk, 0);
    let channels = u32::from(u16_at(chunk, 2));
    let sample_rate = u32_at(chunk, 4);
    let bits = u16_at(chunk, 14);
    if tag == EXTENSIBLE {
        if chunk.len() < EXTENSIBLE_LEN {
            return Err(malformed("the extensible format chunk is too short"));
        }
        tag = u16_at(chunk, SUB_FORMAT_AT);
    }
    if channels == 0 || channels > MAX_CHANNELS {
        return Err(DecodeError::UnsupportedChannelCount(channels));
    }
    if sample_rate == 0 {
        return Err(DecodeError::InvalidSampleRate);
    }
    let encoding = match (tag, bits) {
        (PCM, 8) => Encoding::U8,
        (PCM, 16) => Encoding::S16,
        (PCM, 24) => Encoding::S24,
        (PCM, 32) => Encoding::S32,
        (IEEE_FLOAT, 32) => Encoding::F32,
        (IEEE_FLOAT, 64) => Encoding::F64,
        (ALAW, 8) => Encoding::ALaw,
        (MULAW, 8) => Encoding::MuLaw,
        (PCM | IEEE_FLOAT | ALAW | MULAW, bits) => {
            return Err(DecodeError::Unsupported(format!(
                "{bits}-bit samples in format {tag:#06x}"
            )));
        }
        (tag, _) => {
            return Err(DecodeError::Unsupported(format!("WAV format {tag:#06x}")));
        }
    };
    Ok((encoding, channels, sample_rate))
}
