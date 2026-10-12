use std::sync::Arc;

use super::*;
use crate::filters::{BiquadResonantFilter, DcRemovalFilter, EchoFilter, Filter, FlangerFilter};
use crate::{Bus, Mixer, MixerConfig, Pcm, PlayOptions, Queue, Resampler, Sound};

fn sound(frames: usize, channels: u32, period: usize) -> Sound {
    let samples = (0..frames * channels as usize)
        .map(|i| (i % period) as f32 / period as f32 - 0.5)
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

struct Sources {
    filtered: Sound,
    tone: Sound,
    queued: [Sound; 2],
    echo: Arc<dyn Filter>,
    flanger: Arc<dyn Filter>,
    biquad: BiquadResonantFilter,
}

fn sources() -> Sources {
    let echo: Arc<dyn Filter> = Arc::new(EchoFilter::default());
    let mut filtered = sound(6000, 2, 37);
    filtered.set_filter(0, Some(Arc::clone(&echo)));
    filtered.set_filter(1, Some(Arc::new(DcRemovalFilter::default())));
    Sources {
        filtered,
        tone: sound(9000, 1, 90),
        queued: [sound(700, 1, 23), sound(1500, 2, 61)],
        echo,
        flanger: Arc::new(FlangerFilter::default()),
        biquad: BiquadResonantFilter::default(),
    }
}

#[test]
fn routing_renders_the_same_samples_as_the_mixer() {
    let s = sources();
    let fast = PlayOptions {
        speed: 1.3,
        ..PlayOptions::default()
    };
    let wet = EchoFilter::WET;
    let freq = BiquadResonantFilter::FREQUENCY;

    let mut bus = Bus::new();
    bus.set_filter(0, Some(Arc::clone(&s.flanger)));
    let mut queue = Queue::new();
    queue.set_params(44100.0, 2).unwrap();
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let mut expected = Vec::new();
    {
        let m = &mut mixer;
        m.play(&bus);
        let child = m.bus_play(&bus, &s.filtered, fast);
        let q = m.play(&queue);
        for queued in &s.queued {
            m.queue_play(&queue, queued).unwrap();
        }
        m.set_global_filter(0, Some(&s.biquad));
        let clocked = m.play_clocked(0.25, &s.tone, PlayOptions::default());
        m.play_clocked(0.26, &s.tone, PlayOptions::default());
        let background = m.play_background(&s.tone, Some(0.5), false);
        expected.extend(mix(m, 300));
        m.fade_volume(child, 0.2, 0.01);
        m.oscillate_pan(clocked, -1.0, 1.0, 0.005);
        m.set_filter_parameter(crate::FilterTarget::Voice(child), 0, wet, 0.5);
        m.fade_filter_parameter(crate::FilterTarget::Global, 0, freq, 800.0, 0.01);
        m.schedule_stop(q, 0.03);
        m.seek(child, 0.02).unwrap();
        expected.extend(mix(m, 700));
        m.set_bus_filter(&mut bus, 0, Some(Arc::clone(&s.echo)));
        m.set_bus_resampler(&mut bus, Resampler::Point);
        m.annex_sound(&bus, background);
        m.set_global_filter(0, None);
        expected.extend(mix(m, 500));
        m.stop_sound(&bus);
        expected.extend(mix(m, 300));
    }

    let mut bus = Bus::new();
    bus.set_filter(0, Some(Arc::clone(&s.flanger)));
    let (mut c, mut renderer) = split(
        Mixer::new(MixerConfig::default()).unwrap(),
        RealtimeConfig::default(),
    );
    let r = &mut renderer;
    let mut actual = Vec::new();
    c.play(&bus).unwrap();
    let child = c.bus_play(&bus, &s.filtered, fast).unwrap();
    let q = c.play(&queue).unwrap();
    for queued in &s.queued {
        c.queue_play(&queue, queued).unwrap();
    }
    c.set_global_filter(0, Some(&s.biquad)).unwrap();
    let clocked = c
        .play_clocked(0.25, &s.tone, PlayOptions::default())
        .unwrap();
    c.play_clocked(0.26, &s.tone, PlayOptions::default())
        .unwrap();
    let background = c.play_background(&s.tone, Some(0.5), false).unwrap();
    actual.extend(render(r, 300));
    c.fade_volume(child, 0.2, 0.01).unwrap();
    c.oscillate_pan(clocked, -1.0, 1.0, 0.005).unwrap();
    c.set_filter_parameter(FilterTarget::Voice(child), 0, wet, 0.5)
        .unwrap();
    c.fade_filter_parameter(FilterTarget::Global, 0, freq, 800.0, 0.01)
        .unwrap();
    c.schedule_stop(q, 0.03).unwrap();
    c.seek(child, 0.02).unwrap();
    actual.extend(render(r, 700));
    c.set_bus_filter(&mut bus, 0, Some(Arc::clone(&s.echo)))
        .unwrap();
    c.set_bus_resampler(&mut bus, Resampler::Point).unwrap();
    c.annex_sound(&bus, background).unwrap();
    c.set_global_filter(0, None).unwrap();
    actual.extend(render(r, 500));
    c.stop_sound(&bus).unwrap();
    actual.extend(render(r, 300));

    assert_eq!(actual.len(), expected.len());
    let first = actual
        .iter()
        .zip(&expected)
        .position(|(a, e)| a.to_bits() != e.to_bits());
    assert_eq!(first, None, "first differing sample");
    assert!(expected.iter().any(|&sample| sample != 0.0));
    events(&mut c);
    let status = c.status();
    assert_eq!(
        (status.retired_overflow, status.rejected_queue_sounds),
        (0, 0)
    );
}

#[test]
fn bus_and_queue_events() {
    let s = sources();
    let (mut c, mut renderer) = split(
        Mixer::new(MixerConfig::default()).unwrap(),
        RealtimeConfig::default(),
    );
    let bus = Bus::new();
    let orphan = c.bus_play(&bus, &s.tone, PlayOptions::default()).unwrap();
    let bus_id = c.play(&bus).unwrap();
    let child = c.bus_play(&bus, &s.tone, PlayOptions::default()).unwrap();
    let queue = Queue::new();
    c.queue_play(&queue, &s.queued[0]).unwrap();
    let q = c.play(&queue).unwrap();
    for _ in 0..=crate::QUEUE_MAX {
        c.queue_play(&queue, &s.queued[0]).unwrap();
    }
    render(&mut renderer, 64);
    assert_eq!(
        events(&mut c),
        [
            Event::Failed(orphan),
            Event::Started(bus_id),
            Event::Started(child),
            Event::Started(q),
        ]
    );
    // One sound before the queue played and one past QUEUE_MAX.
    assert_eq!(c.status().rejected_queue_sounds, 2);

    c.stop(bus_id).unwrap();
    c.stop_sound(&queue).unwrap();
    render(&mut renderer, 64);
    let mut finished = events(&mut c);
    finished.sort_by_key(|event| event.id());
    let mut expected = [
        Event::Finished(bus_id, FinishReason::Stopped),
        Event::Finished(child, FinishReason::Stopped),
        Event::Finished(q, FinishReason::Stopped),
    ];
    expected.sort_by_key(|event| event.id());
    assert_eq!(finished, expected);

    // Queued sounds wait for the queue to play again.
    let again = c.play(&queue).unwrap();
    for _ in 0..40 {
        render(&mut renderer, 4096);
    }
    assert_eq!(
        events(&mut c),
        [
            Event::Started(again),
            Event::Finished(again, FinishReason::Ended)
        ]
    );
    for _ in 0..4 {
        render(&mut renderer, 0);
        c.release_retired();
    }
    assert_eq!(c.status().retired_overflow, 0);
    assert_eq!(c.retire_credits, 2 * RealtimeConfig::default().voice_ids);
}

#[test]
fn too_many_queues_are_refused() {
    let config = RealtimeConfig {
        queues: 1,
        ..RealtimeConfig::default()
    };
    let (mut c, _renderer) = split(Mixer::new(MixerConfig::default()).unwrap(), config);
    let first = Queue::new();
    c.play(&first).unwrap();
    c.play(&first).unwrap();
    assert_eq!(c.play(&Queue::new()), Err(ControllerError::TooManyQueues));
}
