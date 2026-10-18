//! The benchmark cases. `reference_bench.cpp` builds each case of the same
//! name the same way on the C++ reference.

use std::sync::Arc;

use solouder_core::filters::{
    BassboostFilter, BiquadResonantFilter, DcRemovalFilter, EchoFilter, FftFilter, Filter,
    FlangerFilter, FreeverbFilter, LofiFilter, RobotizeFilter, WaveShaperFilter,
};
use solouder_core::{Bus, Pcm, PlayOptions, Resampler, Sound};

use crate::cases_io;
use crate::session::{Session, Setup};
use crate::signal::{TONE_RATE, TONES, tone_samples, tones, voice_options};

/// Builds a case's session for a buffer size.
pub type Build = Box<dyn Fn(u32) -> Session>;

/// A named benchmark case.
pub struct Case {
    /// Shared with `reference_bench.cpp` where it has the case.
    pub name: String,
    /// How the C++ number compares, for the report.
    pub reference: Reference,
    /// Builds the session for a buffer size.
    pub build: Build,
}

/// What the C++ reference measures for a case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reference {
    /// The same work: voices, buses, filters and 3D gains.
    Same,
    /// The C++ build decodes streams inside `mix`; Rust decodes on the
    /// stream worker, so the render only copies decoded audio.
    DecodesInMix,
    /// No C++ counterpart.
    None,
}

/// Voice counts of the `voices` cases.
pub const VOICE_COUNTS: [u32; 4] = [16, 64, 256, 1023];

/// Every resampler, by the name the cases use.
pub const RESAMPLERS: [(&str, Resampler); 3] = [
    ("point", Resampler::Point),
    ("linear", Resampler::Linear),
    ("catmullrom", Resampler::CatmullRom),
];

/// Every case, in report order.
pub fn all() -> Vec<Case> {
    let mut cases = voices();
    cases.push(buses());
    cases.push(filters());
    cases.extend(cases_io::streams());
    cases.push(cases_io::spatial());
    #[cfg(feature = "sources")]
    cases.extend(crate::cases_sources::sources());
    cases
}

/// A case built by `build`.
pub fn case(
    name: impl Into<String>,
    reference: Reference,
    build: impl Fn(u32) -> Session + 'static,
) -> Case {
    Case {
        name: name.into(),
        reference,
        build: Box::new(build),
    }
}

/// The plain setup: no stream worker, linear resampling.
pub fn setup(buffer: u32, max_active: u32) -> Setup {
    Setup {
        buffer,
        max_active,
        resampler: Resampler::Linear,
        streams: false,
    }
}

/// `voices{N}_{resampler}`: N looping 44.1 kHz tones, all active.
fn voices() -> Vec<Case> {
    let mut cases = Vec::new();
    for count in VOICE_COUNTS {
        for (label, resampler) in RESAMPLERS {
            let name = format!("voices{count}_{label}");
            cases.push(case(name, Reference::Same, move |buffer| {
                let mut session = Session::new(Setup {
                    resampler,
                    ..setup(buffer, count)
                });
                let tones = tones();
                for i in 0..count as usize {
                    let tone = &tones[i % TONES];
                    session
                        .controller
                        .play_with(tone, voice_options(i))
                        .expect("play");
                }
                session
            }));
        }
    }
    cases
}

/// `buses`: three levels of buses (1, 4 and 16) with 4 tones on each of the
/// 16 leaves: 64 tones and 21 bus voices.
fn buses() -> Case {
    case("buses", Reference::Same, |buffer| {
        let mut session = Session::new(setup(buffer, 128));
        let c = &mut session.controller;
        let tones = tones();
        let top = Bus::new();
        let middle: Vec<Bus> = (0..4).map(|_| Bus::new()).collect();
        let leaves: Vec<Bus> = (0..16).map(|_| Bus::new()).collect();
        c.play(&top).expect("play");
        for bus in &middle {
            c.bus_play(&top, bus, PlayOptions::default()).expect("play");
        }
        for (l, leaf) in leaves.iter().enumerate() {
            c.bus_play(&middle[l / 4], leaf, PlayOptions::default())
                .expect("play");
        }
        for i in 0..64 {
            let tone = &tones[i % TONES];
            c.bus_play(&leaves[i / 4], tone, voice_options(i))
                .expect("play");
        }
        session
    })
}

/// Each filter kind with its default settings, in `reference_bench.cpp`'s
/// order.
pub fn filter_kinds() -> Vec<Arc<dyn Filter>> {
    vec![
        Arc::new(BiquadResonantFilter::default()),
        Arc::new(LofiFilter::default()),
        Arc::new(EchoFilter::default()),
        Arc::new(WaveShaperFilter::default()),
        Arc::new(DcRemovalFilter::default()),
        Arc::new(FftFilter),
        Arc::new(BassboostFilter::default()),
        Arc::new(RobotizeFilter::default()),
        Arc::new(FlangerFilter::default()),
        Arc::new(FreeverbFilter::default()),
    ]
}

/// `filters`: 30 tones, three through each filter kind, and a global
/// biquad lowpass.
fn filters() -> Case {
    case("filters", Reference::Same, |buffer| {
        let mut session = Session::new(setup(buffer, 64));
        let kinds = filter_kinds();
        let sounds: Vec<_> = kinds
            .iter()
            .enumerate()
            .map(|(k, filter)| {
                let pcm =
                    Pcm::from_planar(tone_samples(k % TONES), 1, TONE_RATE).expect("valid tone");
                let mut sound = Sound::new(pcm);
                sound.set_looping(true);
                sound.set_filter(0, Some(Arc::clone(filter)));
                sound
            })
            .collect();
        let c = &mut session.controller;
        c.set_global_filter(0, Some(&BiquadResonantFilter::default()))
            .expect("global filter");
        for i in 0..30 {
            c.play_with(&sounds[i % sounds.len()], voice_options(i))
                .expect("play");
        }
        session
    })
}
