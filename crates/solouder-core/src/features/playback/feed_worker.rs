//! The worker's end of a feed (see [`feed`](super::feed)).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use rtrb::{Consumer, Producer};

use super::feed::{BLOCK, Chunk, FeedShared, LOOPING, Origin};
use super::stream::{StreamReader, StreamSource};

/// The worker's end of a feed: the source, opened on the worker, and the
/// reader it decodes with.
#[derive(Debug)]
pub struct WorkerFeed {
    /// Opened on the first [`service`](Self::service).
    source: Option<Arc<dyn StreamSource>>,
    reader: Option<Box<dyn StreamReader>>,
    shared: Arc<FeedShared>,
    full: Producer<Box<Chunk>>,
    empty: Consumer<Box<Chunk>>,
    /// Chunks in the pool.
    chunks: usize,
    epoch: u32,
    /// The next frame the reader decodes.
    position: u64,
    /// The reader reached the end of the audio.
    ended: bool,
    /// Frames decoded since the last seek, so a loop that yields nothing is
    /// not repeated forever.
    pass_frames: u64,
}

impl WorkerFeed {
    pub(super) fn new(
        origin: Origin,
        shared: Arc<FeedShared>,
        full: Producer<Box<Chunk>>,
        empty: Consumer<Box<Chunk>>,
        chunks: usize,
    ) -> Self {
        let (source, reader, position) = match origin {
            Origin::Source(source) => (Some(source), None, 0),
            Origin::Reader(reader, at) => (None, Some(reader), u64::from(at)),
        };
        Self {
            source,
            reader,
            shared,
            full,
            empty,
            chunks,
            epoch: 0,
            position,
            ended: false,
            // A reader that played before it moved here may loop at once.
            pass_frames: position,
        }
    }

    /// Whether the voice is gone, so the feed (and its file) can be dropped.
    pub fn is_closed(&self) -> bool {
        self.full.is_abandoned()
    }

    /// Opens the source, follows seeks and fills empty chunks, at most a
    /// pool's worth per call so one feed cannot hold up the others. Returns
    /// whether it did anything.
    pub fn service(&mut self) -> bool {
        let mut worked = false;
        if let Some(source) = self.source.take() {
            self.reader = source.open();
            if self.reader.is_none() {
                self.shared.failed.store(true, Ordering::Release);
            }
            worked = true;
        }
        for _ in 0..self.chunks {
            // Checked before every chunk, so a seek is followed at once.
            let request = self.shared.request.load(Ordering::Acquire);
            let epoch = (request >> 32) as u32;
            if epoch != self.epoch {
                self.epoch = epoch;
                self.seek(u64::from(request as u32));
                worked = true;
            }
            if self.reader.is_none() || self.empty.is_empty() {
                break;
            }
            if self.ended {
                let looping = self.shared.looping.load(Ordering::Relaxed);
                if looping & LOOPING == 0 || self.pass_frames == 0 {
                    break;
                }
                self.seek(u64::from(looping as u32));
            }
            let Some(reader) = self.reader.as_mut() else {
                break;
            };
            let Ok(mut chunk) = self.empty.pop() else {
                break;
            };
            let frames = reader.read(&mut chunk.data, BLOCK, BLOCK).min(BLOCK);
            chunk.epoch = self.epoch;
            chunk.start = u32::try_from(self.position).unwrap_or(u32::MAX);
            chunk.frames = frames as u32;
            chunk.end = frames < BLOCK;
            self.position += frames as u64;
            self.pass_frames += frames as u64;
            self.ended = chunk.end;
            // Cannot fail: the ring holds every chunk of the pool.
            let _ = self.full.push(chunk);
            worked = true;
        }
        worked
    }

    /// Rewinds, then skips to `target`; a source that cannot rewind fails.
    fn seek(&mut self, target: u64) {
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        self.ended = false;
        self.pass_frames = 0;
        if !reader.rewind() {
            self.reader = None;
            self.shared.failed.store(true, Ordering::Release);
            return;
        }
        self.position = if target > 0 { reader.skip(target) } else { 0 };
    }
}
