// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT) and
// SoLoud src/core/soloud_file.cpp (zlib/libpng, Copyright (c) 2013-2020
// Jari Komppa); altered for SoLouder.

//! Where stb_vorbis reads its bytes in pull mode: a block of memory
//! (`stb_vorbis_open_memory`) or a `FILE`, which SoLoud maps onto its own
//! `File` class (`soloud_file_hack_on.h`, `Soloud_Filehack_*`). The two
//! paths of `get8`, `getn`, `skip`, `set_file_offset` and
//! `stb_vorbis_get_file_offset` behave differently at the end of the data,
//! and SoLoud's `DiskFile` and `MemoryFile` differ again, so each is
//! reproduced.

use std::io::{ErrorKind, Read, Seek, SeekFrom};

/// The byte-level operations of stb_vorbis that depend on the input. The
/// caller keeps stb's `eof` flag and sets it as the C does.
pub(super) trait Source {
    /// `get8` without its flag: `None` where stb sets `eof` and returns 0.
    fn get8(&mut self) -> Option<u8>;
    /// `getn`: whether all of `data` was read (stb sets `eof` if not).
    fn getn(&mut self, data: &mut [u8]) -> bool;
    /// `skip`: whether stb sets `eof` (only the memory path does).
    fn skip(&mut self, n: i32) -> bool;
    /// `set_file_offset` after it clears `eof`: its result and the new flag.
    fn set_offset(&mut self, loc: u32) -> (bool, bool);
    /// `stb_vorbis_get_file_offset`.
    fn offset(&mut self) -> u32;
}

/// stb's memory path: `stream`, `stream_start` and `stream_end`. The
/// position may pass the end, as stb's pointer does after `skip`.
pub(super) struct Memory<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Memory<'a> {
    /// The whole of `data`, from its first byte.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl Source for Memory<'_> {
    #[inline]
    fn get8(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(byte)
    }

    fn getn(&mut self, data: &mut [u8]) -> bool {
        let end = self.pos.saturating_add(data.len());
        match self.data.get(self.pos..end) {
            Some(bytes) => {
                data.copy_from_slice(bytes);
                self.pos = end;
                true
            }
            None => false,
        }
    }

    fn skip(&mut self, n: i32) -> bool {
        // A pointer before the start would be undefined; stb only steps
        // back over bytes it has just read.
        self.pos = self.pos.saturating_add_signed(n as isize);
        self.pos >= self.data.len()
    }

    fn set_offset(&mut self, loc: u32) -> (bool, bool) {
        if loc as usize >= self.data.len() {
            self.pos = self.data.len();
            (false, true)
        } else {
            self.pos = loc as usize;
            (true, false)
        }
    }

    fn offset(&mut self) -> u32 {
        self.pos as u32
    }
}

/// Which SoLoud `File` a stream reads through.
// Until the streaming reader drives `VorbisFile`, only tests use this.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    /// `DiskFile` (`WavStream::load`): C stdio, whose end-of-file flag is set
    /// by a read that comes up short and cleared by a seek; seeks may pass
    /// the end.
    Disk,
    /// `MemoryFile` (`WavStream::loadMem` and `loadToMem`): at its end once
    /// the position reaches the length, and seeks stop at the last byte.
    Memory,
}

/// Bytes read from the source at a time.
#[cfg_attr(not(test), allow(dead_code))]
const CHUNK: usize = 64 * 1024;

/// SoLoud's `File` over `R`, buffered. Positions are `R`'s, in 32 bits as
/// SoLoud keeps them. A read error is a short read.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct SoloudFile<R> {
    inner: R,
    kind: FileKind,
    pos: u32,
    len: u32,
    /// stdio's end-of-file flag (`Disk` only).
    at_eof: bool,
    buf: Vec<u8>,
    buf_at: u64,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<R: Read + Seek> SoloudFile<R> {
    /// A file at `inner`'s current position. Fails if `inner` cannot tell
    /// its position or length, or if it is 2 GiB or longer (beyond SoLoud's
    /// `int` offsets).
    pub fn new(mut inner: R, kind: FileKind) -> std::io::Result<Self> {
        let pos = inner.stream_position()?;
        let len = inner.seek(SeekFrom::End(0))?;
        if len > i32::MAX as u64 || pos > len {
            return Err(std::io::Error::new(
                ErrorKind::InvalidInput,
                "Ogg Vorbis streams of 2 GiB or more are not supported",
            ));
        }
        Ok(Self {
            inner,
            kind,
            pos: pos as u32,
            len: len as u32,
            at_eof: false,
            buf: Vec::new(),
            buf_at: 0,
        })
    }

    /// Copies bytes from `at` into `dst` as far as the source has them.
    fn fetch(&mut self, at: u64, dst: &mut [u8]) -> usize {
        let mut done = 0;
        while done < dst.len() {
            let at = at + done as u64;
            let end = self.buf_at + self.buf.len() as u64;
            if at >= self.buf_at && at < end {
                let from = (at - self.buf_at) as usize;
                let n = (self.buf.len() - from).min(dst.len() - done);
                dst[done..done + n].copy_from_slice(&self.buf[from..from + n]);
                done += n;
            } else if !self.refill(at) {
                break;
            }
        }
        done
    }

    /// Buffers the bytes from `at`; false if there are none.
    fn refill(&mut self, at: u64) -> bool {
        self.buf.clear();
        self.buf_at = at;
        if self.inner.seek(SeekFrom::Start(at)).is_err() {
            return false;
        }
        self.buf.resize(CHUNK, 0);
        let mut got = 0;
        while got < CHUNK {
            match self.inner.read(&mut self.buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        self.buf.truncate(got);
        got > 0
    }

    /// `File::read`.
    fn read(&mut self, dst: &mut [u8]) -> usize {
        let want = match self.kind {
            FileKind::Disk => dst.len(),
            // `if (mOffset + aBytes >= mDataLength) aBytes = mDataLength - mOffset;`
            FileKind::Memory => dst.len().min(self.len.wrapping_sub(self.pos) as usize),
        };
        let got = self.fetch(u64::from(self.pos), &mut dst[..want]);
        self.pos = self.pos.wrapping_add(got as u32);
        if self.kind == FileKind::Disk && got < dst.len() {
            self.at_eof = true;
        }
        got
    }

    /// `File::eof`.
    fn eof(&self) -> bool {
        match self.kind {
            FileKind::Disk => self.at_eof,
            FileKind::Memory => self.pos >= self.len,
        }
    }

    /// `File::seek`, which `Soloud_Filehack_fseek` calls for every origin.
    fn seek(&mut self, offset: i32) {
        match self.kind {
            FileKind::Disk => {
                // fseek clears the flag even when the position is invalid.
                self.at_eof = false;
                if offset >= 0 {
                    self.pos = offset as u32;
                }
            }
            FileKind::Memory => {
                let pos = if offset >= 0 {
                    offset as u32
                } else {
                    self.len.wrapping_add(offset as u32)
                };
                self.pos = pos.min(self.len.wrapping_sub(1));
            }
        }
    }

    /// `ftell`.
    fn tell(&self) -> i32 {
        self.pos as i32
    }

    /// `fgetc`: `EOF` once the file says so, else one byte, 0 if the read
    /// comes up short (`File::read8`).
    #[inline]
    fn fgetc(&mut self) -> Option<u8> {
        if self.eof() {
            return None;
        }
        let at = u64::from(self.pos);
        let end = self.buf_at + self.buf.len() as u64;
        // Past `eof()`, a buffered byte is one `read` would return in full.
        if at < self.buf_at || at >= end {
            let mut byte = [0];
            self.read(&mut byte);
            return Some(byte[0]);
        }
        self.pos += 1;
        Some(self.buf[(at - self.buf_at) as usize])
    }
}

/// stb's `FILE` path over a SoLoud `File`, from `f_start`.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct FileSource<R> {
    file: SoloudFile<R>,
    f_start: u32,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<R: Read + Seek> FileSource<R> {
    /// `stb_vorbis_open_file`: starts where `file` is and measures the
    /// stream by seeking to the end (`fseek(file, 0, SEEK_END)`), which a
    /// `MemoryFile` stops at its last byte. Returns the source and
    /// `stream_len`.
    pub fn open(mut file: SoloudFile<R>) -> (Self, u32) {
        let start = file.tell() as u32;
        let len = file.len;
        file.seek(len as i32);
        let stream_len = (file.tell() as u32).wrapping_sub(start);
        file.seek(start as i32);
        // stb_vorbis_open_file_section: `p.f_start = (uint32) ftell(file);`
        let f_start = file.tell() as u32;
        (Self { file, f_start }, stream_len)
    }
}

impl<R: Read + Seek> Source for FileSource<R> {
    #[inline]
    fn get8(&mut self) -> Option<u8> {
        self.file.fgetc()
    }

    fn getn(&mut self, data: &mut [u8]) -> bool {
        // `fread(data, n, 1, f)` becomes `read(n) / n`, which divides by
        // zero for an empty read: undefined, so treated as a failed read.
        !data.is_empty() && self.file.read(data) == data.len()
    }

    fn skip(&mut self, n: i32) -> bool {
        let x = self.file.tell();
        self.file.seek(x.wrapping_add(n));
        false
    }

    fn set_offset(&mut self, loc: u32) -> (bool, bool) {
        let (loc, eof) = match loc.checked_add(self.f_start) {
            Some(at) if loc < 0x8000_0000 => (at, false),
            _ => (0x7fff_ffff, true),
        };
        // Soloud_Filehack_fseek always reports success.
        self.file.seek(loc as i32);
        (true, eof)
    }

    fn offset(&mut self) -> u32 {
        (self.file.tell() as u32).wrapping_sub(self.f_start)
    }
}
