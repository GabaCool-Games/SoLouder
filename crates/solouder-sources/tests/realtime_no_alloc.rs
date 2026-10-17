//! Generators through the real-time split: `Renderer::render` must not
//! allocate or free memory while synths play, loop, seek, end and play in
//! a queue, and while a Vic's registers and model and a Monotone's
//! settings change under their voices (noise, Vic, sfxr and Monotone
//! voices, looping and not), and must render exactly what an offline
//! [`Mixer`] given the same calls renders. Generator instances are built
//! when a voice is prepared, on the game thread, so only the reads run on
//! the audio thread.
//!
//! The counting allocator is global, so this file holds a single test.

use std::alloc::System;
use std::sync::Arc;

use solouder_core::realtime::{RealtimeConfig, split};
use solouder_core::{
    Controller, Generator, GeneratorContext, GeneratorInstance, Mixer, MixerConfig, Queue,
    Renderer, Synth,
};
use solouder_sources::{Monotone, Noise, NoiseType, Sfxr, SfxrPreset, Vic, VicModel};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// A finite, rewindable stereo ramp, to wrap loops.
#[derive(Debug)]
struct Ramp {
    frames: usize,
    rewinds: bool,
}

#[derive(Debug)]
struct RampInstance {
    frames: usize,
    rewinds: bool,
    at: usize,
}

impl Generator for Ramp {
    fn channels(&self) -> u32 {
        2
    }

    fn sample_rate(&self) -> f32 {
        22050.0
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        Box::new(RampInstance {
            frames: self.frames,
            rewinds: self.rewinds,
            at: 0,
        })
    }
}

impl GeneratorInstance for RampInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        stride: usize,
        _context: &GeneratorContext,
    ) -> usize {
        let count = frames.min(self.frames - self.at);
        for c in 0..2 {
            for (i, sample) in out[c * stride..][..count].iter_mut().enumerate() {
                *sample = ((self.at + i + 5 * c) % 61) as f32 / 61.0 - 0.5;
            }
        }
        self.at += count;
        count
    }

    fn has_ended(&self) -> bool {
        self.at >= self.frames
    }

    fn rewind(&mut self) -> bool {
        self.at = 0;
        self.rewinds
    }
}

fn noise(kind: NoiseType) -> Synth {
    let noise = Noise::new();
    noise.set_type(kind);
    Synth::new(Arc::new(noise)).unwrap()
}

/// An sfxr sound that restarts inside the source.
fn looping_sfxr() -> Synth {
    let mut synth = Synth::new(Arc::new(Sfxr::from_preset(SfxrPreset::Blip, 2))).unwrap();
    synth.set_looping(true);
    synth
}

/// A Monotone song: four tracks of notes with a slide, an arpeggio and
/// vibrato, at 2 ticks a row.
fn monotone_song() -> Vec<u8> {
    let mut out = b"MONOTONE".to_vec();
    out.extend_from_slice(&[0; 82]);
    out.extend_from_slice(&[1, 1, 4, 2]);
    out.push(0);
    out.extend_from_slice(&[0xff; 255]);
    for row in 0..64_u16 {
        let cells: [u16; 4] = [
            (30 + row % 12) << 9 | 7 << 6 | 2,
            if row % 8 == 0 {
                (40 + row / 8) << 9 | 3 << 6 | 9
            } else {
                0
            },
            if row % 4 == 0 {
                50 << 9 | 3 << 3 | 7
            } else {
                0
            },
            if row % 16 == 0 {
                20 << 9 | 4 << 6 | 3 << 3 | 5
            } else {
                4 << 6
            },
        ];
        out.extend(cells.iter().flat_map(|cell| cell.to_le_bytes()));
    }
    out
}

fn ramp(frames: usize, rewinds: bool, looping: bool) -> Synth {
    let mut synth = Synth::new(Arc::new(Ramp { frames, rewinds })).unwrap();
    synth.set_looping(looping);
    synth.set_loop_point(0.01);
    synth
}

fn measured_render(renderer: &mut Renderer, output: &mut [f32], step: &str) {
    let region = Region::new(GLOBAL);
    renderer.render(output);
    let stats = region.change();
    assert_eq!(
        (stats.allocations, stats.deallocations, stats.reallocations),
        (0, 0, 0),
        "render allocated or freed memory during: {step}"
    );
}

fn between(controller: &mut Controller) {
    while controller.poll_event().is_some() {}
    controller.release_retired();
}

#[test]
fn rendering_synths_never_allocates_and_matches_the_mixer() {
    let config = RealtimeConfig {
        retired_capacity: 2,
        ..RealtimeConfig::default()
    };
    let (mut controller, mut renderer) = split(Mixer::new(MixerConfig::default()).unwrap(), config);
    let mut offline = Mixer::new(MixerConfig::default()).unwrap();
    let mut output = vec![0.0; 2 * 1500];
    let mut expected = vec![0.0; 2 * 1500];

    let vic = Vic::new();
    vic.set_register(Vic::BASS, 220).unwrap();
    let mut chip = Synth::new(Arc::new(vic.clone())).unwrap();
    let song = Monotone::from_bytes(&monotone_song()).unwrap();
    chip.set_looping(true);
    let sounds = [
        chip,
        noise(NoiseType::Pink),
        noise(NoiseType::Blueish),
        ramp(3000, true, true),
        ramp(2000, false, true),
        ramp(700, true, false),
        Synth::new(Arc::new(Sfxr::from_preset(SfxrPreset::Explosion, 5))).unwrap(),
        looping_sfxr(),
        Synth::new(Arc::new(song.clone())).unwrap(),
    ];
    let mut ids = Vec::new();
    let mut handles = Vec::new();
    for sound in &sounds {
        ids.push(controller.play(sound).unwrap());
        handles.push(offline.play(sound));
    }
    let mut queue = Queue::new();
    queue.set_params(44100.0, 2).unwrap();
    controller.play(&queue).unwrap();
    offline.play(&queue);
    let coin = Synth::new(Arc::new(Sfxr::from_preset(SfxrPreset::Coin, 1))).unwrap();
    for sound in [noise(NoiseType::White), ramp(900, true, false), coin] {
        controller.queue_play(&queue, &sound).unwrap();
        offline.queue_play(&queue, &sound).unwrap();
    }
    drop(sounds);
    for round in 0..12 {
        between(&mut controller);
        // Registers change under playing voices, between renders.
        vic.set_register(round % 4, 128 + (round as u8 * 11) % 128)
            .unwrap();
        if round == 7 {
            vic.set_model(VicModel::Ntsc);
        }
        // Hardware channels and the waveform too.
        song.set_params(1 + round as u32 % 4, round as i32 % 9)
            .unwrap();
        if round == 3 {
            // Forward through the generic seek's discard passes, and back
            // with a rewind.
            controller.seek(ids[1], 0.4).unwrap();
            offline.seek(handles[1], 0.4).unwrap();
            controller.seek(ids[3], 0.02).unwrap();
            offline.seek(handles[3], 0.02).unwrap();
            controller.seek(ids[0], 0.3).unwrap();
            offline.seek(handles[0], 0.3).unwrap();
            controller.seek(ids[8], 0.9).unwrap();
            offline.seek(handles[8], 0.9).unwrap();
        }
        let frames = [1500, 700, 333][round % 3];
        measured_render(&mut renderer, &mut output[..2 * frames], "playing synths");
        offline.mix_interleaved(&mut expected[..2 * frames]);
        assert!(
            output[..2 * frames]
                .iter()
                .zip(&expected[..2 * frames])
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "round {round} differs from the offline mixer"
        );
    }
    controller.stop(ids[1]).unwrap();
    measured_render(&mut renderer, &mut output, "stopping");
    between(&mut controller);
}
