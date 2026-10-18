//! The game thread: seeded random requests through the `Controller`, and
//! periodic probes of the active-voice behavior behind upstream #351, #163
//! and #274.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use solouder_bench::signal::TONES;
use solouder_core::{Controller, ControllerError, PlayOptions, VoiceId};

use super::Audio;
use super::sources::{Rng, Sources, sources};
use super::tracker::{Kind, Tracker};

/// `Mixer::set_max_active_voice_count` of the soak (#351's fix was 64).
pub const ACTIVE_LIMIT: u32 = 48;

const PLAIN: Kind = Kind {
    filtered: false,
    spatial: false,
};
const FILTERED: Kind = Kind {
    filtered: true,
    spatial: false,
};
const SPATIAL: Kind = Kind {
    filtered: false,
    spatial: true,
};

/// What the game thread saw.
#[derive(Debug, Default)]
pub struct Report {
    pub frames: u64,
    pub probes: u64,
    pub bursts: u64,
    pub refused: u64,
    /// Most renders between a probe's burst and its last `Started`.
    pub max_start_renders: u64,
}

pub struct Game {
    pub controller: Controller,
    pub tracker: Tracker,
    pub report: Report,
    rng: Rng,
    sources: Sources,
    audio: Arc<Audio>,
}

const DEADLINE: Duration = Duration::from_secs(10);

impl Game {
    pub fn new(controller: Controller, seed: u64, audio: Arc<Audio>) -> Self {
        Self {
            controller,
            tracker: Tracker::default(),
            report: Report::default(),
            rng: Rng::new(seed),
            sources: sources(),
            audio,
        }
    }

    fn drain(&mut self) {
        while let Some(event) = self.controller.poll_event() {
            self.tracker.event(event);
        }
    }

    fn played(&mut self, result: Result<VoiceId, ControllerError>, kind: Kind) {
        match result {
            Ok(id) => self.tracker.played(id, kind),
            Err(ControllerError::QueueFull)
            | Err(ControllerError::NoFreeVoiceId)
            | Err(ControllerError::StreamsFull) => self.report.refused += 1,
            Err(other) => panic!("play refused: {other}"),
        }
    }

    fn sent(&mut self, result: Result<(), ControllerError>) {
        match result {
            Ok(()) => {}
            Err(ControllerError::QueueFull) => self.report.refused += 1,
            Err(other) => panic!("request refused: {other}"),
        }
    }

    fn options(&mut self) -> PlayOptions {
        PlayOptions {
            volume: Some(self.rng.range(0.1, 1.0)),
            pan: self.rng.range(-1.0, 1.0),
            speed: self.rng.range(0.5, 2.0),
            ..PlayOptions::default()
        }
    }

    /// One random request.
    fn step(&mut self) {
        let roll = self.rng.below(100);
        let n = self.rng.next();
        let pick = self.tracker.pick(self.rng.next());
        let options = self.options();
        let c = &mut self.controller;
        let s = &self.sources;
        match roll {
            0..20 => {
                let result = c.play_with(&s.shots[n as usize % TONES], options);
                self.played(result, PLAIN);
            }
            20..30 => {
                let sound = &s.filtered[n as usize % s.filtered.len()];
                let result = c.play_with(sound, options);
                self.played(result, FILTERED);
            }
            30..36 => {
                let result = c.play_with(&s.tones[n as usize % TONES], options);
                self.played(result, PLAIN);
            }
            36..42 => {
                let result = c.play_with(&s.streams[n as usize % s.streams.len()], options);
                self.played(result, PLAIN);
            }
            42..47 => {
                self.report.bursts += 1;
                for i in 0..4 + n % 29 {
                    let shot = &self.sources.shots[i as usize % TONES];
                    let result = self.controller.bus_play(&self.sources.bus, shot, options);
                    self.played(result, PLAIN);
                }
            }
            47..55 => {
                let position = [
                    self.rng.range(-20.0, 20.0),
                    0.0,
                    self.rng.range(-20.0, 20.0),
                ];
                let velocity = [self.rng.range(-10.0, 10.0), 0.0, 0.0];
                let sound = if n.is_multiple_of(3) {
                    &s.tones
                } else {
                    &s.shots
                };
                let sound = &sound[n as usize % TONES];
                let result = c.play_3d(sound, position, velocity, options);
                self.played(result, SPATIAL);
            }
            97..99 => {
                // Playing the bus again replaces its voice.
                let result = c.play(&s.bus);
                self.played(result, PLAIN);
            }
            _ => {
                let Some((id, kind)) = pick else { return };
                let result = match roll {
                    55..65 => c.stop(id),
                    65..69 => c.set_volume(id, options.volume.unwrap_or(1.0)),
                    69..72 => c.set_pan(id, options.pan),
                    72..75 => c.fade_volume(id, 0.0, f64::from(options.speed) * 0.1),
                    75..80 => c.set_pause(id, n.is_multiple_of(2)),
                    80..85 if !kind.filtered => c.set_relative_play_speed(id, options.speed),
                    85..90 => c.seek(id, f64::from(options.speed) * 0.1),
                    90..94 => c.schedule_stop(id, f64::from(options.speed) * 0.1),
                    94..97 => c.set_global_volume(options.volume.unwrap_or(1.0)),
                    _ => Ok(()),
                };
                self.sent(result);
            }
        }
    }

    /// 600 looping tones in one frame: past the voice slots (voices are
    /// stolen) and, with what is already queued, past the command queue
    /// (requests are refused).
    fn flood(&mut self) {
        for i in 0..600 {
            let options = PlayOptions {
                volume: Some(0.01),
                ..PlayOptions::default()
            };
            let tone = &self.sources.tones[i % TONES];
            let result = self.controller.play_with(tone, options);
            self.played(result, PLAIN);
        }
    }

    /// Moves the listener and every 3D voice, and sends an update.
    fn move_3d(&mut self) {
        let ids: Vec<VoiceId> = self.tracker.spatial().collect();
        for id in ids {
            let position = [
                self.rng.range(-20.0, 20.0),
                0.0,
                self.rng.range(-20.0, 20.0),
            ];
            let velocity = [self.rng.range(-30.0, 30.0), 0.0, 0.0];
            self.controller
                .set_3d_source_parameters(id, position, velocity);
        }
        let at = [self.rng.range(-1.0, 1.0), 0.0, -1.0];
        let listener = [0.0; 3];
        self.controller
            .set_3d_listener_parameters(listener, at, [0.0, 1.0, 0.0], listener);
        // Refused while two updates are in flight.
        let _ = self.controller.update_3d_audio();
    }

    /// Renders completed so far.
    fn renders(&self) -> u64 {
        self.controller.status().renders
    }

    /// Waits for `count` more renders, reading events.
    fn wait_renders(&mut self, count: u64) {
        let target = self.renders() + count;
        self.wait("renders", |game| game.renders() >= target);
    }

    fn wait(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            self.drain();
            if done(self) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_micros(500));
        }
    }

    /// Stops everything and reads events until no play is live.
    pub fn settle(&mut self) {
        self.wait("stop_all to be queued", |game| {
            game.controller.stop_all().is_ok()
        });
        self.wait("every voice to finish", |game| game.tracker.live() == 0);
    }

    /// Plays `count` looping tones through the bus in one frame.
    fn burst(&mut self, count: u32, volume: f32) {
        for i in 0..count as usize {
            let options = PlayOptions {
                volume: Some(volume),
                pan: (i % 9) as f32 / 4.0 - 1.0,
                ..PlayOptions::default()
            };
            let tone = &self.sources.tones[i % TONES];
            let id = self.controller.bus_play(&self.sources.bus, tone, options);
            self.tracker.played(id.expect("probe play"), PLAIN);
        }
    }

    /// The quiet-room probe. A burst through a bus up to the active limit
    /// must start within a few renders and be mixed in full (#351: plays
    /// into a bus starting late; #163: valid, non-zero-volume voices that are
    /// silent), audibly and within [-1, 1] (#274). Past the limit, exactly
    /// the limit is mixed: the cause upstream found for #351.
    fn probe(&mut self) {
        self.report.probes += 1;
        self.settle();
        // The random requests may have left the global volume low.
        let volume = self.controller.set_global_volume(1.0);
        self.sent(volume);
        let issued = self.renders();
        let bus = self.controller.play(&self.sources.bus).expect("probe bus");
        self.tracker.played(bus, PLAIN);
        let under = ACTIVE_LIMIT - 1;
        self.burst(under, 1.0 / under as f32);
        self.wait("the burst to start", |game| game.tracker.waiting() == 0);
        let took = self.renders() - issued;
        self.report.max_start_renders = self.report.max_start_renders.max(took);
        assert!(
            took <= 8,
            "a burst of {under} plays took {took} renders to start"
        );
        self.wait_renders(3);
        self.audio.peak.store(0, Ordering::Relaxed);
        self.wait_renders(3);
        let status = self.controller.status();
        assert_eq!(status.voices, ACTIVE_LIMIT, "voices after the burst");
        assert_eq!(
            status.active_voices, ACTIVE_LIMIT,
            "voices left unmixed (#163, #351)"
        );
        let peak = f32::from_bits(self.audio.peak.load(Ordering::Relaxed));
        assert!(peak > 0.05, "the burst is not audible: peak {peak}");

        self.burst(16, 1.0 / under as f32);
        self.wait("the extra plays to start", |game| {
            game.tracker.waiting() == 0
        });
        self.wait_renders(3);
        let status = self.controller.status();
        assert_eq!(status.voices, ACTIVE_LIMIT + 16);
        assert_eq!(status.active_voices, ACTIVE_LIMIT, "the active limit holds");
        self.settle();
    }

    /// Runs random frames until `until`, probing every half second, then
    /// stops everything.
    pub fn run(&mut self, until: Instant) {
        let mut next_probe = Instant::now();
        while Instant::now() < until {
            if Instant::now() >= next_probe {
                self.probe();
                let bus = self.controller.play(&self.sources.bus);
                self.played(bus, PLAIN);
                next_probe = Instant::now() + Duration::from_millis(500);
            }
            self.report.frames += 1;
            self.drain();
            for _ in 0..1 + self.rng.below(3) {
                self.step();
            }
            if self.rng.below(1000) == 0 {
                let stop = self.controller.stop_all();
                self.sent(stop);
            }
            if self.rng.below(150) == 0 {
                self.flood();
            }
            self.move_3d();
            std::thread::sleep(Duration::from_millis(1));
        }
        self.settle();
    }
}
