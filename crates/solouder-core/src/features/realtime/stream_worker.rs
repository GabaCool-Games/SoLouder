//! The thread that decodes streams for a real-time mixer, so the audio
//! thread never reads a file.

use std::thread::JoinHandle;
use std::time::Duration;

use rtrb::Consumer;

use crate::features::playback::WorkerFeed;

/// How long [`StreamWorker::spawn`]'s thread sleeps when every feed is full.
const IDLE: Duration = Duration::from_millis(2);

/// Decodes the streams a [`Controller`](super::Controller) plays, ahead of
/// the audio thread. Take it once with
/// [`Controller::take_stream_worker`](super::Controller::take_stream_worker)
/// and either [`spawn`](Self::spawn) it or call [`service`](Self::service)
/// regularly; streams stay silent until it runs.
///
/// Files are opened, read and closed here.
#[derive(Debug)]
pub struct StreamWorker {
    pub(super) incoming: Consumer<WorkerFeed>,
    pub(super) feeds: Vec<WorkerFeed>,
}

impl StreamWorker {
    /// Takes new streams, opens them, follows seeks and loops, decodes
    /// until each stream's buffer is full, and drops streams whose voices
    /// have ended. Returns whether there was anything to do.
    pub fn service(&mut self) -> bool {
        let mut worked = false;
        while let Ok(feed) = self.incoming.pop() {
            self.feeds.push(feed);
            worked = true;
        }
        let before = self.feeds.len();
        self.feeds.retain(|feed| !feed.is_closed());
        worked |= self.feeds.len() != before;
        for feed in &mut self.feeds {
            worked |= feed.service();
        }
        worked
    }

    /// Streams being decoded.
    pub fn streams(&self) -> usize {
        self.feeds.len()
    }

    /// The controller is gone and no stream is left, so there will be no
    /// more work.
    pub fn is_finished(&self) -> bool {
        self.incoming.is_abandoned() && self.incoming.is_empty() && self.feeds.is_empty()
    }

    /// Services until [`is_finished`](Self::is_finished), sleeping `idle`
    /// whenever there is nothing to do.
    pub fn run(mut self, idle: Duration) {
        while !self.is_finished() {
            if !self.service() {
                std::thread::sleep(idle);
            }
        }
    }

    /// [`run`](Self::run)s on a new thread, which ends once the controller
    /// and the renderer are both gone.
    pub fn spawn(self) -> std::io::Result<JoinHandle<()>> {
        std::thread::Builder::new()
            .name("solouder-streams".to_owned())
            .spawn(move || self.run(IDLE))
    }
}
