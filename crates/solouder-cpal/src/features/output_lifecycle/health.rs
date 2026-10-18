//! What a backend error means for the output, and how a stream that
//! reported a transient error is watched until its callbacks stop.

use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use cpal::ErrorKind;

use super::callback::OutputShared;

pub(crate) const PLAYING: u8 = 0;
pub(crate) const DISCONNECTED: u8 = 1;
pub(crate) const FAILED: u8 = 2;

/// The shortest silence after a transient error that counts as a stopped
/// stream; longer device buffers wait four callbacks' worth.
pub(crate) const STALL: Duration = Duration::from_millis(500);

/// What the output does about an error the backend reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Recoverable by design: an underrun, a device change the stream
    /// followed, a refused real-time priority. Counted only.
    Ignore,
    /// May or may not have stopped the stream: an error the backend could
    /// not classify, or a busy device. Counted; the output stays playing
    /// while callbacks keep arriving (ALSA's PulseAudio plugin reports an
    /// `EIO` from `snd_pcm_avail_delay` about once per stream and plays on).
    Watch,
    /// The device is gone or the stream must be rebuilt.
    Disconnect,
    /// Anything else, which cpal says to treat as permanent.
    Fail,
}

pub(crate) fn effect(kind: ErrorKind) -> Effect {
    match kind {
        ErrorKind::DeviceChanged | ErrorKind::Xrun | ErrorKind::RealtimeDenied => Effect::Ignore,
        ErrorKind::BackendError | ErrorKind::DeviceBusy => Effect::Watch,
        ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated => Effect::Disconnect,
        _ => Effect::Fail,
    }
}

/// Records an error from the backend's error callback.
pub(crate) fn report(shared: &OutputShared, kind: ErrorKind) {
    shared.errors.fetch_add(1, Ordering::Relaxed);
    let state = match effect(kind) {
        Effect::Ignore => return,
        Effect::Watch => {
            shared.watched_errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
        Effect::Disconnect => DISCONNECTED,
        Effect::Fail => FAILED,
    };
    // The first final state stays.
    let _ = shared
        .state
        .compare_exchange(PLAYING, state, Ordering::Relaxed, Ordering::Relaxed);
}

/// After a watched error, whether callbacks have stopped. Checked when the
/// output's state is read, so noticing takes two reads at least the stall
/// timeout apart.
pub(crate) fn stalled(shared: &OutputShared, watch: &Mutex<Watch>, rate: u32) -> bool {
    if shared.watched_errors.load(Ordering::Relaxed) == 0 {
        return false;
    }
    let timeout = stall_timeout(shared.callback_frames.load(Ordering::Relaxed), rate);
    let callbacks = shared.callbacks.load(Ordering::Relaxed);
    let mut watch = watch.lock().unwrap_or_else(PoisonError::into_inner);
    watch.stalled(callbacks, Instant::now(), timeout)
}

/// The callback count last seen, and since when it has not moved.
#[derive(Debug)]
pub(crate) struct Watch {
    seen: u64,
    since: Instant,
}

impl Watch {
    pub fn new(now: Instant) -> Self {
        Self {
            seen: 0,
            since: now,
        }
    }

    /// Whether no callback has arrived for `timeout`, given the callback
    /// count `callbacks` at `now`.
    pub fn stalled(&mut self, callbacks: u64, now: Instant, timeout: Duration) -> bool {
        if callbacks != self.seen {
            self.seen = callbacks;
            self.since = now;
            return false;
        }
        now.duration_since(self.since) >= timeout
    }
}

/// How long callbacks may pause before a watched stream counts as stopped:
/// [`STALL`], or four callbacks of `frames` frames at `rate` if longer.
pub(crate) fn stall_timeout(frames: u64, rate: u32) -> Duration {
    let callback = Duration::from_secs_f64(frames as f64 / f64::from(rate.max(1)));
    STALL.max(callback * 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_count_every_error_and_keep_the_first_final_state() {
        let shared = OutputShared::default();
        report(&shared, ErrorKind::Xrun);
        report(&shared, ErrorKind::BackendError);
        report(&shared, ErrorKind::BackendError);
        assert_eq!(shared.state.load(Ordering::Relaxed), PLAYING);
        assert_eq!(shared.watched_errors.load(Ordering::Relaxed), 2);
        report(&shared, ErrorKind::StreamInvalidated);
        report(&shared, ErrorKind::Other);
        assert_eq!(shared.state.load(Ordering::Relaxed), DISCONNECTED);
        assert_eq!(shared.errors.load(Ordering::Relaxed), 5);
        // Without a watched error the output never stalls.
        let quiet = OutputShared::default();
        let watch = Mutex::new(Watch::new(Instant::now() - Duration::from_secs(9)));
        assert!(!stalled(&quiet, &watch, 48_000));
        assert!(stalled(&shared, &watch, 48_000));
    }

    #[test]
    fn only_unclassified_and_busy_errors_are_watched() {
        assert_eq!(effect(ErrorKind::Xrun), Effect::Ignore);
        assert_eq!(effect(ErrorKind::DeviceChanged), Effect::Ignore);
        assert_eq!(effect(ErrorKind::BackendError), Effect::Watch);
        assert_eq!(effect(ErrorKind::DeviceBusy), Effect::Watch);
        assert_eq!(effect(ErrorKind::StreamInvalidated), Effect::Disconnect);
        assert_eq!(effect(ErrorKind::Other), Effect::Fail);
        assert_eq!(effect(ErrorKind::PermissionDenied), Effect::Fail);
    }

    #[test]
    fn a_watched_stream_stalls_only_when_callbacks_stop() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut watch = Watch::new(start);
        let timeout = STALL;
        assert!(!watch.stalled(0, at(100), timeout));
        assert!(!watch.stalled(5, at(400), timeout));
        assert!(!watch.stalled(9, at(1_000), timeout));
        assert!(!watch.stalled(9, at(1_499), timeout));
        assert!(watch.stalled(9, at(1_500), timeout));
        assert!(!watch.stalled(10, at(1_600), timeout));
        // A stream that never called back stalls from its start.
        assert!(Watch::new(start).stalled(0, at(500), timeout));
    }

    #[test]
    fn long_buffers_wait_four_callbacks() {
        assert_eq!(stall_timeout(512, 48_000), STALL);
        assert_eq!(stall_timeout(48_000, 48_000), Duration::from_secs(4));
        assert_eq!(stall_timeout(0, 0), STALL);
    }
}
