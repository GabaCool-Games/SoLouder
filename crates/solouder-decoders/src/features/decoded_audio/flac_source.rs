//! The bytes of a FLAC file as Symphonia reads them: metadata checked
//! first, and STREAMINFO adjusted where Symphonia misreads it.

use std::io::{self, Read, Seek, SeekFrom};

use symphonia::core::io::MediaSource;

use super::DecodeError;

/// A FLAC file whose metadata blocks all fit in it, with up to two bytes
/// replaced as it is read.
pub(crate) struct FlacSource<S> {
    inner: S,
    len: u64,
    position: u64,
    /// Bytes shown instead of the file's, and where.
    overlay: Option<(u64, [u8; 2])>,
}

impl<S: Read + Seek> FlacSource<S> {
    /// Checks the metadata block headers of the FLAC file in `inner`: each
    /// block must end inside the file, so a damaged header cannot make the
    /// decoder reserve memory for data that is not there.
    ///
    /// Symphonia takes a stream whose STREAMINFO gives equal minimum and
    /// maximum block sizes for one of fixed block sizes, and rejects frames
    /// numbered by sample. A stream of variable block sizes may still have
    /// blocks of one size; its minimum is shown one lower so it decodes.
    pub(crate) fn new(mut inner: S) -> Result<Self, DecodeError> {
        let len = inner.seek(SeekFrom::End(0)).map_err(DecodeError::Io)?;
        inner.seek(SeekFrom::Start(0)).map_err(DecodeError::Io)?;
        let mut tag = [0; 4];
        read(&mut inner, &mut tag)?;
        if &tag != b"fLaC" {
            return Err(DecodeError::NotRecognized);
        }
        let mut at = 4_u64;
        let mut sizes = None;
        loop {
            let mut header = [0; 4];
            read(&mut inner, &mut header)?;
            let size = u64::from(u32::from_be_bytes([0, header[1], header[2], header[3]]));
            if at + 4 + size > len {
                return Err(DecodeError::Malformed(
                    "a FLAC metadata block runs past the end of the file".to_owned(),
                ));
            }
            if header[0] & 0x7F == 0 && size >= 4 && sizes.is_none() {
                let mut block = [0; 4];
                read(&mut inner, &mut block)?;
                let min = u16::from_be_bytes([block[0], block[1]]);
                let max = u16::from_be_bytes([block[2], block[3]]);
                sizes = Some((at + 4, min, max));
            }
            at += 4 + size;
            inner.seek(SeekFrom::Start(at)).map_err(DecodeError::Io)?;
            if header[0] & 0x80 != 0 {
                break;
            }
        }
        let mut sync = [0; 2];
        let variable = inner.read_exact(&mut sync).is_ok() && sync == [0xFF, 0xF9];
        let overlay = match sizes {
            Some((offset, min, max)) if variable && min == max && max > 1 => {
                Some((offset, (max - 1).to_be_bytes()))
            }
            _ => None,
        };
        inner.seek(SeekFrom::Start(0)).map_err(DecodeError::Io)?;
        Ok(Self {
            inner,
            len,
            position: 0,
            overlay,
        })
    }
}

fn read(inner: &mut impl Read, buffer: &mut [u8]) -> Result<(), DecodeError> {
    inner
        .read_exact(buffer)
        .map_err(|_| DecodeError::Malformed("FLAC metadata cut short".to_owned()))
}

impl<S: Read> Read for FlacSource<S> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buffer)?;
        if let Some((offset, bytes)) = self.overlay {
            for (i, byte) in bytes.iter().enumerate() {
                let at = offset + i as u64;
                if (self.position..self.position + n as u64).contains(&at) {
                    buffer[(at - self.position) as usize] = *byte;
                }
            }
        }
        self.position += n as u64;
        Ok(n)
    }
}

impl<S: Seek> Seek for FlacSource<S> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.position = self.inner.seek(to)?;
        Ok(self.position)
    }
}

impl<S: Read + Seek + Send + Sync> MediaSource for FlacSource<S> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}
