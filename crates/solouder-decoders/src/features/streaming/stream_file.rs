//! Where a [`WavStream`](super::WavStream) reads its file from
//! (SoLoud's `File`, as passed to `WavStream::loadFile`).

use std::any::Any;
use std::fmt;
use std::fs::File;
use std::io::{self, Cursor, Read, Seek};
use std::path::PathBuf;
use std::sync::Arc;

/// A readable, seekable view of a stream's file, one per voice.
pub trait StreamInput: Read + Seek + Send {}

impl<T: Read + Seek + Send> StreamInput for T {}

/// A file a [`WavStream`](super::WavStream) can open once per voice
/// ([`WavStream::from_file`](super::WavStream::from_file), SoLoud's
/// `WavStream::loadFile`). SoLoud keeps the caller's one `File` and every
/// voice seeks the same cursor, so two voices corrupt each other's reads;
/// here each voice opens its own [`StreamInput`].
///
/// Implemented for paths (opened again for each voice) and for bytes held
/// in memory.
pub trait StreamFile: Any + Send + Sync + fmt::Debug {
    /// Opens the file at its start, for one voice.
    fn open(&self) -> io::Result<Box<dyn StreamInput>>;
}

impl StreamFile for PathBuf {
    fn open(&self) -> io::Result<Box<dyn StreamInput>> {
        Ok(Box::new(File::open(self)?))
    }
}

impl StreamFile for Arc<[u8]> {
    fn open(&self) -> io::Result<Box<dyn StreamInput>> {
        Ok(Box::new(Cursor::new(Arc::clone(self))))
    }
}
