//! Feeds: a streaming voice's audio, decoded ahead on a worker thread so
//! the audio thread never reads a file.
//!
//! A feed is a pool of chunks, each one block of frames, passed between the
//! two threads through a pair of rings: full chunks to the audio thread,
//! empty ones back. Each chunk carries the epoch it was decoded for and the
//! source frame it starts at. A seek on the audio thread that lands where
//! the queued audio continues keeps it; any other starts a new epoch and asks
//! the worker for the frame, and chunks of older epochs are handed back
//! unread. The worker also continues from the loop point when the voice
//! loops, so a loop wrap finds its audio waiting.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

use super::stream::{StreamReader, StreamSource};
use crate::contracts::SAMPLE_GRANULARITY;

const BLOCK: usize = SAMPLE_GRANULARITY as usize;
const LOOPING: u64 = 1 << 63;

/// One block of decoded frames, channel `c` at `c * BLOCK`.
#[derive(Debug)]
pub(crate) struct Chunk {
    epoch: u32,
    /// The source frame of the first frame.
    start: u32,
    frames: u32,
    /// The audio ended after these frames.
    end: bool,
    data: Box<[f32]>,
}

/// What the two sides of a feed share.
#[derive(Debug)]
struct FeedShared {
    /// `epoch << 32 | frame`: where the audio thread wants the audio to
    /// continue.
    request: AtomicU64,
    /// [`LOOPING`] when the voice loops, with the frame a loop wrap
    /// continues from.
    looping: AtomicU64,
    /// The source could not be opened, or stopped seeking.
    failed: AtomicBool,
    /// Frames of silence played because a feed ran dry, for every feed of
    /// one worker.
    underflow: Arc<AtomicU64>,
}

/// Makes a feed of `chunks` chunks for a stream of `channels` channels:
/// the audio thread's end and the worker's.
pub(crate) fn feed(
    source: Arc<dyn StreamSource>,
    channels: u32,
    chunks: usize,
    underflow: Arc<AtomicU64>,
) -> (FeedReader, WorkerFeed) {
    let chunks = chunks.max(2);
    let shared = Arc::new(FeedShared {
        request: AtomicU64::new(0),
        looping: AtomicU64::new(0),
        failed: AtomicBool::new(false),
        underflow,
    });
    let (full_tx, full_rx) = RingBuffer::new(chunks);
    let (mut empty_tx, empty_rx) = RingBuffer::new(chunks);
    for _ in 0..chunks {
        let chunk = Chunk {
            epoch: 0,
            start: 0,
            frames: 0,
            end: false,
            data: vec![0.0; channels as usize * BLOCK].into_boxed_slice(),
        };
        // Cannot fail: the ring holds every chunk.
        let _ = empty_tx.push(Box::new(chunk));
    }
    let reader = FeedReader {
        shared: Arc::clone(&shared),
        full: full_rx,
        empty: empty_tx,
        current: None,
        cursor: 0,
        epoch: 0,
        primed: false,
    };
    let worker = WorkerFeed {
        source,
        reader: None,
        opened: false,
        shared,
        full: full_tx,
        empty: empty_rx,
        epoch: 0,
        position: 0,
        ended: false,
        pass_frames: 0,
    };
    (reader, worker)
}

/// The audio thread's end of a feed. Nothing here allocates, frees or
/// waits.
#[derive(Debug)]
pub(crate) struct FeedReader {
    shared: Arc<FeedShared>,
    full: Consumer<Box<Chunk>>,
    empty: Producer<Box<Chunk>>,
    current: Option<Box<Chunk>>,
    /// Frames of `current` already read.
    cursor: u32,
    epoch: u32,
    /// A chunk of this epoch has arrived: silence from now on is underflow,
    /// not prebuffering.
    primed: bool,
}

impl FeedReader {
    /// The source could not be opened (or seeked): the voice has ended.
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Acquire)
    }

    /// Tells the worker whether the voice loops, and from which frame.
    pub fn set_looping(&self, from: Option<u32>) {
        let value = from.map_or(0, |frame| LOOPING | u64::from(frame));
        self.shared.looping.store(value, Ordering::Relaxed);
    }

    fn recycle(&mut self, chunk: Box<Chunk>) {
        // Cannot fail: the ring has room for every chunk of the pool.
        let _ = self.empty.push(chunk);
    }

    /// Hands back chunks of older epochs; true if a chunk of this one is
    /// next.
    fn drop_stale(&mut self) -> bool {
        while let Ok(chunk) = self.full.peek() {
            if chunk.epoch == self.epoch {
                return true;
            }
            if let Ok(chunk) = self.full.pop() {
                self.recycle(chunk);
            }
        }
        false
    }

    /// Moves to the next chunk of this epoch, if one has arrived.
    fn advance(&mut self) -> bool {
        // The worker can refill a chunk as soon as it is read.
        if let Some(old) = self.current.take() {
            self.recycle(old);
        }
        if !self.drop_stale() {
            return false;
        }
        let Ok(chunk) = self.full.pop() else {
            return false;
        };
        if let Some(old) = self.current.replace(chunk) {
            self.recycle(old);
        }
        self.cursor = 0;
        self.primed = true;
        true
    }

    /// Copies up to `frames` frames into `out` (`(samples, start, stride)`:
    /// channel `c` at `c * stride + start`), or skips them without `out`.
    /// Returns the frames read and whether the audio ended there; frames the
    /// feed did not have yet are silent.
    pub fn read(
        &mut self,
        mut out: Option<(&mut [f32], usize, usize)>,
        frames: u32,
        channels: u32,
    ) -> (u32, bool) {
        let mut done = 0;
        let mut ended = false;
        while done < frames {
            let (left, end) = match &self.current {
                Some(chunk) => (chunk.frames - self.cursor, chunk.end),
                None => (0, false),
            };
            if left == 0 {
                if end {
                    ended = true;
                    break;
                }
                if !self.advance() {
                    break;
                }
                continue;
            }
            let count = left.min(frames - done);
            if let (Some(chunk), Some((block, start, stride))) = (&self.current, &mut out) {
                for c in 0..channels as usize {
                    let from = c * BLOCK + self.cursor as usize;
                    let to = c * *stride + *start + done as usize;
                    block[to..to + count as usize]
                        .copy_from_slice(&chunk.data[from..from + count as usize]);
                }
            }
            self.cursor += count;
            done += count;
        }
        if !ended && done < frames {
            if let Some((block, start, stride)) = out {
                for c in 0..channels as usize {
                    let to = c * stride + start;
                    block[to + done as usize..to + frames as usize].fill(0.0);
                }
            }
            if self.primed {
                let missing = u64::from(frames - done);
                self.shared.underflow.fetch_add(missing, Ordering::Relaxed);
            }
        }
        (done, ended)
    }

    /// Continues from source frame `target`: keeps the queued audio if it
    /// continues there, and asks the worker for the frame otherwise.
    pub fn seek_to(&mut self, target: u32) {
        let next = match &self.current {
            Some(chunk) if self.cursor < chunk.frames => {
                Some(chunk.start.saturating_add(self.cursor))
            }
            _ => None,
        };
        if next == Some(target) {
            return;
        }
        let queued = next.is_none() && self.drop_stale();
        if queued && self.full.peek().is_ok_and(|chunk| chunk.start == target) {
            self.advance();
            return;
        }
        self.epoch = self.epoch.wrapping_add(1);
        if let Some(chunk) = self.current.take() {
            self.recycle(chunk);
        }
        self.drop_stale();
        self.primed = false;
        let request = u64::from(self.epoch) << 32 | u64::from(target);
        self.shared.request.store(request, Ordering::Release);
    }
}

/// The worker's end of a feed: the source, opened on the worker, and the
/// reader it decodes with.
#[derive(Debug)]
pub struct WorkerFeed {
    source: Arc<dyn StreamSource>,
    reader: Option<Box<dyn StreamReader>>,
    opened: bool,
    shared: Arc<FeedShared>,
    full: Producer<Box<Chunk>>,
    empty: Consumer<Box<Chunk>>,
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
    /// Whether the voice is gone, so the feed (and its file) can be dropped.
    pub fn is_closed(&self) -> bool {
        self.full.is_abandoned()
    }

    /// Opens the source, follows seeks and fills empty chunks. Returns
    /// whether it did anything.
    pub fn service(&mut self) -> bool {
        if !self.opened {
            self.opened = true;
            self.reader = self.source.open();
            if self.reader.is_none() {
                self.shared.failed.store(true, Ordering::Release);
            }
            return true;
        }
        if self.reader.is_none() {
            return false;
        }
        let request = self.shared.request.load(Ordering::Acquire);
        let epoch = (request >> 32) as u32;
        let mut worked = false;
        if epoch != self.epoch {
            self.epoch = epoch;
            self.seek(u64::from(request as u32));
            worked = true;
        }
        while !self.empty.is_empty() {
            if self.ended {
                let looping = self.shared.looping.load(Ordering::Relaxed);
                if looping & LOOPING == 0 || self.pass_frames == 0 {
                    break;
                }
                self.seek(u64::from(looping as u32));
            }
            let Ok(mut chunk) = self.empty.pop() else {
                break;
            };
            let Some(reader) = self.reader.as_mut() else {
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
