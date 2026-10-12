//! Filters first running at a layout they were not prepared for, mixers
//! split after offline use, and voices ending outside a mix.

use std::sync::Arc;

use super::*;
use crate::filters::{EchoFilter, FftFilter, Filter, FlangerFilter};
use crate::{Bus, Mixer, MixerConfig, Pcm, PlayOptions, Queue, Sound};

fn sound(frames: usize, channels: u32) -> Sound {
    let samples = (0..frames * channels as usize)
        .map(|i| 0.4 + (i % 29) as f32 / 290.0)
        .collect();
    Sound::new(Pcm::from_planar(samples, channels, 44100.0).unwrap())
}

fn render(renderer: &mut Renderer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    renderer.render(&mut out);
    out
}

fn mix(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    mixer.mix_interleaved(&mut out);
    out
}

fn events(controller: &mut Controller) -> Vec<Event> {
    std::iter::from_fn(|| controller.poll_event()).collect()
}

fn assert_same(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    let first = actual
        .iter()
        .zip(expected)
        .position(|(a, e)| a.to_bits() != e.to_bits());
    assert_eq!(first, None, "first differing sample");
    assert!(expected.iter().any(|&sample| sample != 0.0));
}

/// Delays lowered before a filter's first block, and a bus whose channel
/// setting changed while it plays, render as they do offline.
#[test]
fn filters_first_running_at_another_layout_match_the_mixer() {
    let long = sound(20000, 2);
    let flanger: Arc<dyn Filter> = Arc::new(FlangerFilter::default());
    let echo = EchoFilter::default();
    let fast = PlayOptions {
        speed: 2.0,
        ..PlayOptions::default()
    };

    let mut bus = Bus::new();
    let mut m = Mixer::new(MixerConfig::default()).unwrap();
    let mut expected = Vec::new();
    m.play_with(&bus, fast);
    m.bus_play(&bus, &long, PlayOptions::default());
    m.play(&long);
    expected.extend(mix(&mut m, 300));
    bus.set_channels(1).unwrap();
    m.set_bus_filter(&mut bus, 0, Some(Arc::clone(&flanger)));
    m.set_bus_filter(&mut bus, 1, Some(Arc::new(FftFilter)));
    m.set_global_filter(0, Some(&*flanger));
    m.set_filter_parameter(crate::FilterTarget::Global, 0, FlangerFilter::DELAY, 0.001);
    m.set_global_filter(1, Some(&echo));
    m.set_filter_parameter(crate::FilterTarget::Global, 1, EchoFilter::DELAY, 0.02);
    expected.extend(mix(&mut m, 900));
    m.set_filter_parameter(crate::FilterTarget::Global, 0, FlangerFilter::DELAY, 0.004);
    expected.extend(mix(&mut m, 900));

    let mut bus = Bus::new();
    let (mut c, mut r) = split(
        Mixer::new(MixerConfig::default()).unwrap(),
        RealtimeConfig::default(),
    );
    let mut actual = Vec::new();
    c.play_with(&bus, fast).unwrap();
    c.bus_play(&bus, &long, PlayOptions::default()).unwrap();
    c.play(&long).unwrap();
    actual.extend(render(&mut r, 300));
    bus.set_channels(1).unwrap();
    c.set_bus_filter(&mut bus, 0, Some(Arc::clone(&flanger)))
        .unwrap();
    c.set_bus_filter(&mut bus, 1, Some(Arc::new(FftFilter)))
        .unwrap();
    c.set_global_filter(0, Some(&*flanger)).unwrap();
    c.set_filter_parameter(FilterTarget::Global, 0, FlangerFilter::DELAY, 0.001)
        .unwrap();
    c.set_global_filter(1, Some(&echo)).unwrap();
    c.set_filter_parameter(FilterTarget::Global, 1, EchoFilter::DELAY, 0.02)
        .unwrap();
    actual.extend(render(&mut r, 900));
    c.set_filter_parameter(FilterTarget::Global, 0, FlangerFilter::DELAY, 0.004)
        .unwrap();
    actual.extend(render(&mut r, 900));

    assert_same(&actual, &expected);
    events(&mut c);
    assert_eq!(c.status().retired_overflow, 0);
}

/// A mixer split after offline use already holds queue storage, queued
/// sounds and voices; the controller's retirement accounting covers them.
#[test]
fn a_mixer_used_offline_splits_with_room_for_what_it_holds() {
    let mut queue = Queue::new();
    queue.set_params(44100.0, 2).unwrap();
    queue.set_filter(0, Some(Arc::new(FftFilter)));
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.play(&queue);
    mixer.queue_play(&queue, &sound(100, 2)).unwrap();
    // The queue runs dry and its voice stops; its storage stays.
    mix(&mut mixer, 512);
    mixer.play(&queue);
    mixer.queue_play(&queue, &sound(100, 2)).unwrap();
    mixer.play(&sound(30000, 1));

    let config = RealtimeConfig {
        voice_ids: 1,
        retired_capacity: 1,
        ..RealtimeConfig::default()
    };
    let (mut c, mut r) = split(mixer, config);
    let q = c.play(&queue).unwrap();
    c.set_global_filter(0, None).unwrap();
    render(&mut r, 0);
    c.release_retired();
    c.set_global_filter(0, None).unwrap();
    c.stop(q).unwrap();
    render(&mut r, 64);
    c.stop_all().unwrap();
    for _ in 0..4 {
        render(&mut r, 64);
        events(&mut c);
    }
    assert_eq!(c.status().retired_overflow, 0);
}

/// A plain `Mixer::play` after `keep_retired` gets room to retire its voice.
#[test]
fn kept_retirement_covers_voices_played_afterwards() {
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.keep_retired(8);
    let handle = mixer.play(&sound(1000, 1));
    mixer.stop(handle);
    let mut kept = 0;
    mixer.drain_retired(8, |_| kept += 1);
    assert_eq!((kept, mixer.retired_overflow()), (1, 0));
}

/// A voice a bus seek plays to its end reports `Ended`, even when a play in
/// the same render takes its slot.
#[test]
fn voices_a_seek_plays_out_report_ended() {
    let (mut c, mut r) = split(
        Mixer::new(MixerConfig::default()).unwrap(),
        RealtimeConfig::default(),
    );
    let bus = Bus::new();
    let bus_id = c.play(&bus).unwrap();
    let child = c
        .bus_play(&bus, &sound(2000, 1), PlayOptions::default())
        .unwrap();
    render(&mut r, 64);
    events(&mut c);
    c.seek(bus_id, 1.0).unwrap();
    let next = c.play(&sound(5000, 1)).unwrap();
    render(&mut r, 64);
    assert_eq!(
        events(&mut c),
        [
            Event::Finished(child, FinishReason::Ended),
            Event::Started(next)
        ]
    );
}
