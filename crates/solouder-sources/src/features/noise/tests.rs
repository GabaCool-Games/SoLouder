use std::sync::Arc;

use solouder_core::{Mixer, MixerConfig, Synth};

use super::*;

fn render(instance: &mut dyn GeneratorInstance, frames: usize) -> Vec<f32> {
    let mut out = vec![f32::NAN; frames];
    let context = GeneratorContext::new(false, 44100.0);
    assert_eq!(
        instance.get_audio(&mut out, frames, frames, &context),
        frames
    );
    out
}

#[test]
fn presets_set_their_weights() {
    let noise = Noise::new();
    assert_eq!(noise.octave_scale()[..2], [1.0, 0.0]);
    noise.set_type(NoiseType::from_index(2));
    assert_eq!(noise.octave_scale()[9], 10.0);
    noise.set_type(NoiseType::from_index(3));
    assert_eq!(noise.octave_scale()[0], 10.0);
    noise.set_type(NoiseType::from_index(99));
    assert_eq!(noise.octave_scale(), NoiseType::White.octave_scale());
}

#[test]
fn white_noise_adds_the_first_octave() {
    let mut prg = Prg::new(0xfade);
    let out = render(&mut *Noise::new().instance(), 64);
    for &sample in &out {
        assert!((-1.0..=1.0).contains(&sample));
    }
    assert_eq!(out[0], prg.rand_float() - 0.5);
}

#[test]
fn every_instance_starts_from_the_same_seed() {
    let noise = Noise::new();
    noise.set_type(NoiseType::Pink);
    let a = render(&mut *noise.instance(), 512);
    let b = render(&mut *noise.instance(), 512);
    assert_eq!(a, b);
}

#[test]
fn weights_summing_to_zero_give_silence() {
    let noise = Noise::new();
    noise.set_octave_scale([1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert!(
        render(&mut *noise.instance(), 512)
            .iter()
            .all(|&s| s == 0.0)
    );
    noise.set_octave_scale([0.0; OCTAVES]);
    assert!(
        render(&mut *noise.instance(), 512)
            .iter()
            .all(|&s| s == 0.0)
    );
}

#[test]
fn changed_weights_reach_later_voices() {
    let noise = Arc::new(Noise::new());
    let synth = Synth::new(noise.clone()).unwrap();
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let first = mixer.play(&synth);
    noise.set_type(NoiseType::Blueish);
    let second = mixer.play(&synth);
    let mut out = vec![0.0; 2 * 256];
    mixer.mix_interleaved(&mut out);
    assert!(mixer.is_valid_voice_handle(first) && mixer.is_valid_voice_handle(second));
    assert_eq!(mixer.count_sound(&synth), 2);
    assert!(out.iter().any(|&s| s != 0.0));
}
