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

use super::feed_worker::WorkerFeed;
use super::stream::{StreamInput, StreamReader, StreamSource};
use super::voice::VoiceSource;
use crate::contracts::SAMPLE_GRANULARITY;

pub(super) const BLOCK: usize = SAMPLE_GRANULARITY as usize;
pub(super) const LOOPING: u64 = 1 << 63;

/// One block of decoded frames, channel `c` at `c * BLOCK`.
#[derive(Debug)]
pub(super) struct Chunk {
    pub epoch: u32,
    /// The source frame of the first frame.
    pub start: u32,
    pub frames: u32,
    /// The audio ended after these frames.
    pub end: bool,
    pub data: Box<[f32]>,
}

/// What the two sides of a feed share.
#[derive(Debug)]
pub(super) struct FeedShared {
    /// `epoch << 32 | frame`: where the audio thread wants the audio to
    /// continue.
    pub request: AtomicU64,
    /// [`LOOPING`] when the voice loops, with the frame a loop wrap
    /// continues from.
    pub looping: AtomicU64,
    /// The source could not be opened, or stopped seeking.
    pub failed: AtomicBool,
    /// Frames of silence played because a feed ran dry, for every feed of
    /// one worker.
    pub underflow: Arc<AtomicU64>,
}

/// Where a feed's worker gets its audio.
pub(super) enum Origin {
    /// Opened on the worker.
    Source(Arc<dyn StreamSource>),
    /// Already open, at this frame.
    Reader(Box<dyn StreamReader>, u32),
}

/// Makes a feed of `chunks` chunks for a stream of `channels` channels:
/// the audio thread's end and the worker's.
pub(super) fn feed(
    origin: Origin,
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
    let position = match &origin {
        Origin::Source(_) => 0,
        Origin::Reader(_, at) => *at,
    };
    let reader = FeedReader {
        shared: Arc::clone(&shared),
        full: full_rx,
        empty: empty_tx,
        chunks,
        current: None,
        cursor: 0,
        epoch: 0,
        position,
        padded: 0,
        primed: false,
    };
    let worker = WorkerFeed::new(origin, shared, full_tx, empty_rx, chunks);
    (reader, worker)
}

/// Moves a stream voice that reads on the mixing thread onto a feed,
/// keeping its reader and place, and returns the worker's end. `looping` is
/// the frame a loop wrap continues from, if the voice loops.
pub(crate) fn feed_direct(
    source: &mut VoiceSource,
    chunks: usize,
    underflow: &Arc<AtomicU64>,
    looping: Option<u32>,
) -> Option<WorkerFeed> {
    let VoiceSource::Stream {
        input,
        channels,
        offset,
        ..
    } = source
    else {
        return None;
    };
    let reader = match std::mem::replace(input, StreamInput::Missing) {
        StreamInput::Direct(reader) => reader,
        other => {
            *input = other;
            return None;
        }
    };
    let origin = Origin::Reader(reader, *offset);
    let (reader, worker) = feed(origin, *channels, chunks, Arc::clone(underflow));
    reader.set_looping(looping);
    *input = StreamInput::Feed(Box::new(reader));
    Some(worker)
}

/// The audio thread's end of a feed. Nothing here allocates, frees or
/// waits.
#[derive(Debug)]
pub(crate) struct FeedReader {
    shared: Arc<FeedShared>,
    full: Consumer<Box<Chunk>>,
    empty: Producer<Box<Chunk>>,
    /// Chunks in the pool.
    chunks: usize,
    current: Option<Box<Chunk>>,
    /// Frames of `current` already read.
    cursor: u32,
    epoch: u32,
    /// The source frame read next.
    position: u32,
    /// Frames of silence played since the last seek because the audio had
    /// not arrived.
    padded: u64,
    /// A chunk of this epoch has arrived: silence from now on is underflow,
    /// not prebuffering.
    primed: bool,
}

impl FeedReader {
    /// The source could not be opened (or seeked), or the worker is gone
    /// and its audio has been played: the voice has ended.
    pub fn failed(&self) -> bool {
        if self.shared.failed.load(Ordering::Acquire) {
            return true;
        }
        let drained = self
            .current
            .as_ref()
            .is_none_or(|c| self.cursor >= c.frames);
        self.full.is_abandoned() && self.full.is_empty() && drained
    }

    /// Tells the worker whether the voice loops, and from which frame.
    pub fn set_looping(&self, from: Option<u32>) {
        let value = from.map_or(0, |frame| LOOPING | u64::from(frame));
        self.shared.looping.store(value, Ordering::Relaxed);
    }

    /// Frames of silence played since the last seek while the audio had not
    /// arrived: the voice's stream position ran ahead of its audio by this
    /// much.
    pub fn padded(&self) -> u64 {
        self.padded
    }

    fn recycle(&mut self, chunk: Box<Chunk>) {
        // Cannot fail: the ring has room for every chunk of the pool.
        let _ = self.empty.push(chunk);
    }

    /// Hands back chunks of older epochs; true if a chunk of this one is
    /// next. Gives up after a pool's worth: the worker may still be making
    /// chunks of an older epoch.
    fn drop_stale(&mut self) -> bool {
        for _ in 0..self.chunks {
            let Ok(chunk) = self.full.peek() else {
                return false;
            };
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
        self.position = chunk.start;
        self.current = Some(chunk);
        self.cursor = 0;
        self.primed = true;
        true
    }

    /// Copies up to `frames` frames into `out` (`(samples, start, stride)`:
    /// channel `c` at `c * stride + start`), or skips them without `out`.
    /// Returns the frames read and whether the audio ended there. Frames
    /// the feed does not have yet are silent; skipping past them asks the
    /// worker for the frame after them.
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
            self.position = self.position.saturating_add(count);
            done += count;
        }
        if !ended && done < frames {
            let missing = frames - done;
            match out {
                Some((block, start, stride)) => {
                    for c in 0..channels as usize {
                        let to = c * stride + start;
                        block[to + done as usize..to + frames as usize].fill(0.0);
                    }
                    self.padded += u64::from(missing);
                    if self.primed {
                        let underflow = &self.shared.underflow;
                        underflow.fetch_add(u64::from(missing), Ordering::Relaxed);
                    }
                }
                None => {
                    self.seek_to(self.position.saturating_add(missing));
                    done = frames;
                }
            }
        }
        (done, ended)
    }

    /// Continues from source frame `target`: keeps the queued audio if it
    /// continues there, and asks the worker for the frame otherwise.
    pub fn seek_to(&mut self, target: u32) {
        self.padded = 0;
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
        self.position = target;
        self.primed = false;
        if let Some(chunk) = self.current.take() {
            self.recycle(chunk);
        }
        // Published before the stale chunks are handed back, so the worker
        // stops making them.
        let request = u64::from(self.epoch) << 32 | u64::from(target);
        self.shared.request.store(request, Ordering::Release);
        self.drop_stale();
    }
}
