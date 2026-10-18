//! Tracks every play from its request to its final event, and fails on any
//! event out of place: each play reports `Started` or `Failed`, a started
//! voice reports exactly one `Finished`, and no ID is handed out twice
//! while live.

use std::collections::BTreeMap;

use solouder_core::{Event, FinishReason, VoiceId};

/// What a live voice is, for choosing safe operations on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    /// Has filters: speed changes after its first block could make the
    /// renderer allocate (see port-status "Filter sizing"), so none are sent.
    pub filtered: bool,
    /// Played in 3D: moved between updates.
    pub spatial: bool,
}

#[derive(Clone, Copy, Debug)]
struct Live {
    kind: Kind,
    started: bool,
}

/// Every play in flight, and totals.
#[derive(Debug, Default)]
pub struct Tracker {
    live: BTreeMap<VoiceId, Live>,
    /// Play requests accepted by the controller.
    pub plays: u64,
    /// `Started` events.
    pub started: u64,
    /// `Failed` events.
    pub failed: u64,
    /// `Finished` events, by reason: ended, stopped, stolen.
    pub finished: [u64; 3],
}

impl Tracker {
    /// Records an accepted play request.
    pub fn played(&mut self, id: VoiceId, kind: Kind) {
        let previous = self.live.insert(
            id,
            Live {
                kind,
                started: false,
            },
        );
        assert!(previous.is_none(), "{id:?} was handed out while still live");
        self.plays += 1;
    }

    /// Checks and records an event.
    pub fn event(&mut self, event: Event) {
        match event {
            Event::Started(id) => {
                let live = self.live.get_mut(&id);
                let live = live.unwrap_or_else(|| panic!("Started for unknown {id:?}"));
                assert!(!live.started, "{id:?} started twice");
                live.started = true;
                self.started += 1;
            }
            Event::Failed(id) => {
                let live = self.live.remove(&id);
                let live = live.unwrap_or_else(|| panic!("Failed for unknown {id:?}"));
                assert!(!live.started, "{id:?} failed after it started");
                self.failed += 1;
            }
            Event::Finished(id, reason) => {
                let live = self.live.remove(&id);
                let live = live.unwrap_or_else(|| panic!("Finished for unknown {id:?}"));
                assert!(live.started, "{id:?} finished without starting");
                let slot = match reason {
                    FinishReason::Ended => 0,
                    FinishReason::Stopped => 1,
                    FinishReason::Stolen => 2,
                };
                self.finished[slot] += 1;
            }
        }
    }

    /// Final events seen.
    pub fn finals(&self) -> u64 {
        self.failed + self.finished.iter().sum::<u64>()
    }

    /// Voices requested and not yet finished.
    pub fn live(&self) -> usize {
        self.live.len()
    }

    /// Live voices that have not reported `Started` yet.
    pub fn waiting(&self) -> usize {
        self.live.values().filter(|live| !live.started).count()
    }

    /// The `n`th live voice (wrapping) with its kind, if any.
    pub fn pick(&self, n: u64) -> Option<(VoiceId, Kind)> {
        if self.live.is_empty() {
            return None;
        }
        let (&id, live) = self.live.iter().nth(n as usize % self.live.len())?;
        Some((id, live.kind))
    }

    /// Live 3D voices.
    pub fn spatial(&self) -> impl Iterator<Item = VoiceId> + '_ {
        let spatial = self.live.iter().filter(|(_, live)| live.kind.spatial);
        spatial.map(|(&id, _)| id)
    }
}
