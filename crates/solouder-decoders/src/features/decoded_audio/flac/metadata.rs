// Ported from dr_flac 0.12.31 by David Reid (public domain or MIT No
// Attribution), as patched for SoLouder's C++ reference; altered for SoLouder.

//! The `fLaC` tag and metadata blocks before the first FLAC frame
//! (`drflac__init_private__native` and `drflac__read_and_decode_metadata`,
//! opened strictly and without a metadata callback, as SoLoud opens files).

use std::io::{Read, Seek, SeekFrom};

use crate::DecodeError;

/// What the STREAMINFO block declares, and where the frames start.
#[derive(Clone, Copy, Debug)]
pub(super) struct StreamInfo {
    pub min_block: u32,
    pub max_block: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub sample_bits: u32,
    /// 0 when unknown.
    pub frames: u64,
    /// Byte offset of the first frame.
    pub first_frame: u64,
}

/// Reads the metadata of the FLAC file in `source`, from its start.
/// Metadata blocks are skipped; one that runs past the end of the file is
/// rejected, where dr_flac would stop at it and find no frames.
pub(super) fn read<R: Read + Seek>(source: &mut R) -> Result<StreamInfo, DecodeError> {
    let len = source.seek(SeekFrom::End(0)).map_err(DecodeError::Io)?;
    source.seek(SeekFrom::Start(0)).map_err(DecodeError::Io)?;
    let mut tag = [0; 4];
    read_exact(source, &mut tag)?;
    if &tag != b"fLaC" {
        return Err(malformed("not a FLAC file"));
    }
    let (mut last, kind, size) = block_header(source)?;
    if kind != 0 || size != 34 {
        return Err(malformed("the first FLAC metadata block is not STREAMINFO"));
    }
    let mut info = [0; 34];
    read_exact(source, &mut info)?;
    let sizes = u32::from_be_bytes([info[0], info[1], info[2], info[3]]);
    let props = u64::from_be_bytes([
        info[10], info[11], info[12], info[13], info[14], info[15], info[16], info[17],
    ]);
    let mut position = 42_u64;
    while !last {
        let (is_last, kind, size) = block_header(source)?;
        last = is_last;
        // dr_flac rejects these blocks when they are too short to parse.
        let least = match kind {
            2 => 4,
            4 => 8,
            5 => 396,
            6 => 32,
            _ => 0,
        };
        if size < least {
            return Err(malformed("a FLAC metadata block is too short"));
        }
        position += 4 + u64::from(size);
        if position > len {
            return Err(malformed(
                "a FLAC metadata block runs past the end of the file",
            ));
        }
        source
            .seek(SeekFrom::Start(position))
            .map_err(DecodeError::Io)?;
    }
    Ok(StreamInfo {
        min_block: sizes >> 16,
        max_block: sizes & 0xFFFF,
        sample_rate: (props >> 44) as u32,
        channels: ((props >> 41) & 7) as u32 + 1,
        sample_bits: ((props >> 36) & 0x1F) as u32 + 1,
        frames: props & 0xF_FFFF_FFFF,
        first_frame: position,
    })
}

/// A metadata block header: whether it is the last, its type and its size.
fn block_header<R: Read>(source: &mut R) -> Result<(bool, u8, u32), DecodeError> {
    let mut header = [0; 4];
    read_exact(source, &mut header)?;
    let size = u32::from_be_bytes([0, header[1], header[2], header[3]]);
    Ok((header[0] & 0x80 != 0, header[0] & 0x7F, size))
}

fn read_exact<R: Read>(source: &mut R, out: &mut [u8]) -> Result<(), DecodeError> {
    source.read_exact(out).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            malformed("the FLAC metadata is cut off")
        } else {
            DecodeError::Io(error)
        }
    })
}

fn malformed(message: &str) -> DecodeError {
    DecodeError::Malformed(message.to_owned())
}
