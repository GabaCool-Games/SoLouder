// Ported from SoLoud src/audiosource/wav/soloud_wav.cpp and
// src/core/soloud_file.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Decoding a file read through a reader (`Wav::loadFile`, which reads the
//! whole `File` into memory with `MemoryFile::openFileToMem` first).

use std::io::Read;

use solouder_core::Pcm;

use super::{DecodeError, DecodeLimits, decode};

/// Decodes the audio file `reader` holds, read to its end first, as
/// SoLoud's `Wav::loadFile` does; the format is chosen as for [`decode`].
/// Reading stops with [`DecodeError::FileTooLarge`] past
/// [`DecodeLimits::max_file_bytes`], so an endless reader cannot exhaust
/// memory.
pub fn decode_reader(reader: impl Read, limits: &DecodeLimits) -> Result<Pcm, DecodeError> {
    let bytes = read_capped(reader, limits.max_file_bytes())?;
    decode(&bytes, limits)
}

/// Reads `reader` to its end, failing past `limit` bytes.
pub(crate) fn read_capped(reader: impl Read, limit: u64) -> Result<Vec<u8>, DecodeError> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(DecodeError::Io)?;
    if bytes.len() as u64 > limit {
        return Err(DecodeError::FileTooLarge { limit });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor};

    use super::*;

    fn wav(frames: u32) -> Vec<u8> {
        let data = frames * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&8000u32.to_le_bytes());
        out.extend_from_slice(&16000u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        out.extend((0..frames).flat_map(|i| ((i % 300) as i16 * 100).to_le_bytes()));
        out
    }

    /// Hands out a few bytes per read, as pipes and sockets do.
    struct Trickle(Cursor<Vec<u8>>);

    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(3);
            self.0.read(&mut buf[..n])
        }
    }

    #[test]
    fn readers_decode_as_their_bytes_do() {
        let bytes = wav(500);
        let limits = DecodeLimits::default();
        let direct = decode(&bytes, &limits).unwrap();
        let read = decode_reader(Trickle(Cursor::new(bytes)), &limits).unwrap();
        assert_eq!(read.channel(0), direct.channel(0));
        assert_eq!(read.sample_rate(), 8000.0);
    }

    #[test]
    fn readers_stop_past_the_byte_limit() {
        let limits = DecodeLimits { max_samples: 100 };
        let endless = io::repeat(0);
        assert!(matches!(
            decode_reader(endless, &limits),
            Err(DecodeError::FileTooLarge { limit }) if limit == limits.max_file_bytes()
        ));
        let exact = vec![0; limits.max_file_bytes() as usize];
        assert_eq!(
            read_capped(&exact[..], limits.max_file_bytes())
                .unwrap()
                .len(),
            exact.len()
        );
    }

    #[test]
    fn empty_and_failing_readers_fail() {
        let limits = DecodeLimits::default();
        assert!(decode_reader(&[][..], &limits).is_err());
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("broken"))
            }
        }
        assert!(matches!(
            decode_reader(Broken, &limits),
            Err(DecodeError::Io(_))
        ));
    }
}
