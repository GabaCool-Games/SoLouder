//! Cases of the generated sources (`solouder-sources`, feature `sources`):
//! `sources_{kind}`, 16 looping voices of one generator each, resampled from
//! the generator's rate to the output's. `reference_bench.cpp` builds the
//! same voices from the same settings and fixtures.

use std::sync::Arc;

use solouder_core::dsp::waveform;
use solouder_core::{Generator, Synth};
use solouder_sources::{
    Ay, Monotone, Noise, NoiseType, Sfxr, SfxrPreset, Speech, TedSid, Vic, Vizsn,
};

use crate::cases::{Case, Reference, case, setup};
use crate::session::Session;
use crate::signal::{fixture, voice_options};

/// Voices per `sources_*` case.
pub const SOURCE_VOICES: usize = 16;

/// The text the speech cases read.
pub const SPEECH_TEXT: &str = "The quick brown fox jumps over the lazy dog.";

/// The text the Vizsn case reads.
pub const VIZSN_TEXT: &str = "hyv\u{e4}\u{e4} p\u{e4}iv\u{e4}\u{e4}, mit\u{e4} kuuluu";

fn looping(generator: impl Generator + 'static) -> Synth {
    let mut synth = Synth::new(Arc::new(generator)).expect("a playable generator");
    synth.set_looping(true);
    synth
}

/// The looping generators of case `kind`, one of [`KINDS`]; voice `i`
/// plays `synths[i % len]`.
pub fn synths(kind: &str) -> Vec<Synth> {
    let dump = |path: &str| std::fs::read(fixture(path)).expect("fixture");
    match kind {
        "noise" => {
            let noise = Noise::new();
            noise.set_type(NoiseType::Pink);
            vec![looping(noise)]
        }
        "vic" => {
            let vic = Vic::new();
            for (register, value) in [(Vic::BASS, 200), (Vic::ALTO, 210), (Vic::SOPRANO, 220)] {
                vic.set_register(register, value).expect("a register");
            }
            vec![looping(vic)]
        }
        "sfxr" => (0..7)
            .map(|p| {
                let preset = SfxrPreset::from_index(p).expect("a preset");
                looping(Sfxr::from_preset(preset, p as i32 + 1))
            })
            .collect(),
        "monotone" => {
            let song = Monotone::from_bytes(&dump("monotone/demo.mon")).expect("a song");
            song.set_params(4, waveform::SQUARE).expect("params");
            vec![looping(song)]
        }
        "tedsid" => vec![looping(
            TedSid::from_bytes(&dump("chipdump/sidted.zx7.dump")).expect("a dump"),
        )],
        "ay" => vec![looping(
            Ay::from_bytes(&dump("chipdump/ay_ay.dump")).expect("a dump"),
        )],
        "speech" => vec![looping(Speech::new(SPEECH_TEXT))],
        // The text in CP437, as Vizsn reads it: ä is 0x84.
        "vizsn" => {
            let text: Vec<u8> = VIZSN_TEXT
                .chars()
                .map(|c| if c == '\u{e4}' { 0x84 } else { c as u8 })
                .collect();
            vec![looping(Vizsn::new(text))]
        }
        other => unreachable!("no source case {other}"),
    }
}

/// The source kinds, in report order.
pub const KINDS: [&str; 8] = [
    "noise", "vic", "sfxr", "monotone", "tedsid", "ay", "speech", "vizsn",
];

/// `sources_{kind}` for each kind.
pub fn sources() -> Vec<Case> {
    KINDS
        .iter()
        .map(|&kind| {
            case(format!("sources_{kind}"), Reference::Same, move |buffer| {
                let mut session = Session::new(setup(buffer, SOURCE_VOICES as u32));
                let synths = synths(kind);
                for i in 0..SOURCE_VOICES {
                    session
                        .controller
                        .play_with(&synths[i % synths.len()], voice_options(i))
                        .expect("play");
                }
                session
            })
        })
        .collect()
}
