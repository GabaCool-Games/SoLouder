use std::sync::Arc;

use crate::contracts::Pcm;
use crate::dsp::Clipper;
use crate::features::filters::{BiquadResonantFilter, BiquadType, LofiFilter, WaveShaperFilter};
use crate::features::playback::{FilterTarget, Handle, Mixer, MixerConfig, Sound};

fn mixer() -> Mixer {
    let config = MixerConfig {
        clipper: Clipper::Hard,
        channels: 1,
        ..MixerConfig::default()
    };
    let mut mixer = Mixer::new(config).unwrap();
    mixer.set_post_clip_scaler(1.0);
    mixer
}

fn square(rate: f32) -> Sound {
    let samples = (0..44100)
        .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
        .collect();
    Sound::new(Pcm::from_planar(samples, 1, rate).unwrap())
}

fn lowpass() -> Arc<BiquadResonantFilter> {
    let mut biquad = BiquadResonantFilter::default();
    biquad.set_params(BiquadType::Lowpass, 200.0, 0.7).unwrap();
    Arc::new(biquad)
}

fn peak(mixer: &mut Mixer, frames: usize) -> f32 {
    let mut out = vec![0.0; frames];
    mixer.mix_interleaved(&mut out);
    out[frames / 2..]
        .iter()
        .fold(0.0, |peak, x| peak.max(x.abs()))
}

#[test]
fn a_sound_filter_runs_on_its_voices() {
    let mut sound = square(44100.0);
    sound.set_filter(3, Some(lowpass()));
    let mut mixer = mixer();
    let handle = mixer.play(&sound);
    assert!(peak(&mut mixer, 2048) < 0.01);
    mixer.set_filter_parameter(
        FilterTarget::Voice(handle),
        3,
        BiquadResonantFilter::WET,
        0.0,
    );
    assert!(peak(&mut mixer, 2048) > 0.35);
}

#[test]
fn voices_filter_at_their_own_rate() {
    let mut sound = square(1000.0);
    sound.set_filter(0, Some(lowpass()));
    let mut mixer = mixer();
    let handle = mixer.play(&sound);
    peak(&mut mixer, 4096);
    // The cutoff is limited below the voice's Nyquist frequency of 500 Hz,
    // whatever the output rate.
    let target = FilterTarget::Voice(handle);
    mixer.set_filter_parameter(target, 0, BiquadResonantFilter::FREQUENCY, 20_000.0);
    assert_eq!(
        mixer.filter_parameter(target, 0, BiquadResonantFilter::FREQUENCY),
        Some(20_000.0)
    );
    assert!(peak(&mut mixer, 4096).is_finite());
}

#[test]
fn each_voice_has_its_own_instance() {
    let mut sound = square(44100.0);
    sound.set_filter(0, Some(lowpass()));
    let mut mixer = mixer();
    let first = FilterTarget::Voice(mixer.play(&sound));
    let second = FilterTarget::Voice(mixer.play(&sound));
    mixer.set_filter_parameter(first, 0, BiquadResonantFilter::WET, 0.5);
    assert_eq!(mixer.filter_parameter(first, 0, 0), Some(0.5));
    assert_eq!(mixer.filter_parameter(second, 0, 0), Some(1.0));
    sound.set_filter(0, None);
    assert_eq!(mixer.filter_parameter(second, 0, 0), Some(1.0));
    let third = FilterTarget::Voice(mixer.play(&sound));
    assert_eq!(mixer.filter_parameter(third, 0, 0), None);
}

#[test]
fn global_filters_shape_the_whole_mix() {
    let mut mixer = mixer();
    mixer.play(&square(44100.0));
    mixer.set_global_filter(7, Some(lowpass().as_ref()));
    assert!(peak(&mut mixer, 2048) < 0.01);
    mixer.set_global_filter(7, None);
    assert!(peak(&mut mixer, 2048) > 0.35);
    mixer.set_global_filter(8, Some(lowpass().as_ref()));
    assert_eq!(mixer.filter_parameter(FilterTarget::Global, 8, 0), None);
}

#[test]
fn parameter_fades_run_on_stream_time() {
    let mut mixer = mixer();
    mixer.set_global_filter(0, Some(&WaveShaperFilter::default()));
    let amount = WaveShaperFilter::AMOUNT;
    mixer.fade_filter_parameter(FilterTarget::Global, 0, amount, 0.5, 1.0);
    let mut out = vec![0.0; 22050];
    mixer.mix_interleaved(&mut out);
    let value = mixer
        .filter_parameter(FilterTarget::Global, 0, amount)
        .unwrap();
    // Faders update at the start of each mix, after the stream time advances.
    assert!((value - 0.25).abs() < 1e-6, "{value}");
    mixer.oscillate_filter_parameter(FilterTarget::Global, 0, 0, 0.0, 1.0, 1.0);
    mixer.mix_interleaved(&mut out);
    assert!(mixer.filter_parameter(FilterTarget::Global, 0, 0).unwrap() < 1.0);
}

#[test]
fn group_targets_change_every_member() {
    let mut sound = square(44100.0);
    sound.set_filter(1, Some(Arc::new(LofiFilter::default())));
    let mut mixer = mixer();
    let group = mixer.create_voice_group();
    for _ in 0..2 {
        let voice = mixer.play(&sound);
        mixer.add_voice_to_group(group, voice).unwrap();
    }
    let target = FilterTarget::Voice(group);
    mixer.set_filter_parameter(target, 1, LofiFilter::BIT_DEPTH, 8.0);
    assert_eq!(
        mixer.filter_parameter(target, 1, LofiFilter::BIT_DEPTH),
        Some(8.0)
    );
    let stale = FilterTarget::Voice(Handle::INVALID);
    mixer.set_filter_parameter(stale, 1, 0, 0.0);
    assert_eq!(mixer.filter_parameter(stale, 1, 0), None);
}
