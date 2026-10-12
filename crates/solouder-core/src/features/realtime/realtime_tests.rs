use super::*;
use crate::{Mixer, MixerConfig, Pcm, PlayOptions, Sound};

fn sound(frames: usize) -> Sound {
    let samples = (0..frames).map(|i| (i % 50) as f32 / 50.0 - 0.5).collect();
    Sound::new(Pcm::from_planar(samples, 1, 44100.0).unwrap())
}

fn runtime(config: RealtimeConfig) -> (Controller, Renderer) {
    split(Mixer::new(MixerConfig::default()).unwrap(), config)
}

fn render(renderer: &mut Renderer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    renderer.render(&mut out);
    out
}

fn events(controller: &mut Controller) -> Vec<Event> {
    std::iter::from_fn(|| controller.poll_event()).collect()
}

#[test]
fn a_voice_starts_plays_and_finishes() {
    let (mut controller, mut renderer) = runtime(RealtimeConfig::default());
    let id = controller.play(&sound(1000)).unwrap();
    assert_eq!(controller.voice_ids_in_use(), 1);
    let out = render(&mut renderer, 512);
    assert!(out.iter().any(|&s| s != 0.0));
    assert_eq!(events(&mut controller), [Event::Started(id)]);
    assert_eq!(controller.status().voices, 1);
    render(&mut renderer, 512);
    render(&mut renderer, 512);
    assert_eq!(
        events(&mut controller),
        [Event::Finished(id, FinishReason::Ended)]
    );
    assert_eq!(controller.voice_ids_in_use(), 0);
    let status = controller.status();
    assert_eq!((status.voices, status.renders), (0, 3));
    assert!((status.stream_time - 1536.0 / 44100.0).abs() < 1e-6);
}

#[test]
fn renders_the_same_samples_as_the_mixer() {
    let tone = sound(3000);
    let options = PlayOptions {
        volume: Some(0.7),
        pan: 0.3,
        paused: false,
    };
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let handle = mixer.play_with(&tone, options);
    let mut expected = vec![0.0; 2 * 700];
    mixer.mix_interleaved(&mut expected[..600]);
    mixer.set_relative_play_speed(handle, 1.5).unwrap();
    mixer.mix_interleaved(&mut expected[600..]);

    let (mut controller, mut renderer) = runtime(RealtimeConfig::default());
    let id = controller.play_with(&tone, options).unwrap();
    let mut actual = render(&mut renderer, 300);
    controller.set_relative_play_speed(id, 1.5).unwrap();
    actual.extend(render(&mut renderer, 400));
    assert_eq!(actual, expected);
}

#[test]
fn a_stop_queued_before_the_first_render_still_reports_both_events() {
    let (mut controller, mut renderer) = runtime(RealtimeConfig::default());
    let id = controller.play(&sound(10_000)).unwrap();
    controller.stop(id).unwrap();
    let out = render(&mut renderer, 256);
    assert!(out.iter().all(|&s| s == 0.0));
    assert_eq!(
        events(&mut controller),
        [
            Event::Started(id),
            Event::Finished(id, FinishReason::Stopped)
        ]
    );
}

#[test]
fn commands_for_finished_voices_do_nothing() {
    let (mut controller, mut renderer) = runtime(RealtimeConfig {
        voice_ids: 1,
        ..RealtimeConfig::default()
    });
    let old = controller.play(&sound(100)).unwrap();
    render(&mut renderer, 512);
    assert_eq!(events(&mut controller).len(), 2);
    let new = controller.play(&sound(10_000)).unwrap();
    assert_eq!(new.index(), old.index());
    controller.stop(old).unwrap();
    controller.set_volume(old, 0.0).unwrap();
    let out = render(&mut renderer, 256);
    assert!(out.iter().any(|&s| s != 0.0));
    assert_eq!(events(&mut controller), [Event::Started(new)]);
}

#[test]
fn stop_all_finishes_every_voice() {
    let (mut controller, mut renderer) = runtime(RealtimeConfig::default());
    let ids: Vec<_> = (0..3)
        .map(|_| controller.play(&sound(10_000)).unwrap())
        .collect();
    render(&mut renderer, 64);
    controller.stop_all().unwrap();
    render(&mut renderer, 64);
    let finished: Vec<_> = events(&mut controller)
        .into_iter()
        .filter(|event| matches!(event, Event::Finished(_, FinishReason::Stopped)))
        .map(Event::id)
        .collect();
    assert_eq!(finished.len(), 3);
    assert!(ids.iter().all(|id| finished.contains(id)));
    assert_eq!(controller.status().voices, 0);
}

#[test]
fn full_queues_and_id_pools_reject_requests() {
    let (mut controller, mut renderer) = runtime(RealtimeConfig {
        voice_ids: 2,
        command_capacity: 2,
        ..RealtimeConfig::default()
    });
    let tone = sound(10_000);
    let a = controller.play(&tone).unwrap();
    controller.set_pan(a, 1.0).unwrap();
    assert_eq!(controller.play(&tone), Err(ControllerError::QueueFull));
    assert_eq!(controller.voice_ids_in_use(), 1);
    render(&mut renderer, 64);
    controller.play(&tone).unwrap();
    assert_eq!(controller.play(&tone), Err(ControllerError::NoFreeVoiceId));
    assert_eq!(
        controller.set_relative_play_speed(a, 0.0),
        Err(ControllerError::InvalidSpeed)
    );
}

#[test]
fn renders_apply_a_bounded_number_of_commands() {
    let (mut controller, mut renderer) = runtime(RealtimeConfig {
        commands_per_render: 2,
        ..RealtimeConfig::default()
    });
    let tone = sound(10_000);
    for _ in 0..5 {
        controller.play(&tone).unwrap();
    }
    render(&mut renderer, 64);
    assert_eq!(events(&mut controller).len(), 2);
    assert_eq!(controller.status().deferred_commands, 3);
    render(&mut renderer, 64);
    render(&mut renderer, 64);
    assert_eq!(events(&mut controller).len(), 3);
    assert_eq!(controller.status().deferred_commands, 0);
}

#[test]
fn finished_audio_is_released_by_the_controller() {
    let (mut controller, mut renderer) = runtime(RealtimeConfig::default());
    let tone = sound(100);
    controller.play(&tone).unwrap();
    render(&mut renderer, 512);
    // The request's copy of the sound and the voice's samples.
    assert_eq!(controller.release_retired_audio(), 2);
    assert_eq!(controller.status().retired_overflow, 0);
}

#[test]
fn a_full_retirement_queue_holds_audio_in_the_renderer() {
    let (mut controller, mut renderer) = runtime(RealtimeConfig {
        retired_capacity: 1,
        ..RealtimeConfig::default()
    });
    controller.play(&sound(100)).unwrap();
    render(&mut renderer, 512);
    assert_eq!(controller.release_retired_audio(), 1);
    render(&mut renderer, 64);
    assert_eq!(controller.release_retired_audio(), 1);
    assert_eq!(controller.status().retired_overflow, 0);
}

#[test]
fn play_fails_when_every_voice_is_protected() {
    let (mut controller, mut renderer) = runtime(RealtimeConfig::default());
    let tone = sound(100);
    let paused = PlayOptions {
        paused: true,
        ..PlayOptions::default()
    };
    for _ in 0..crate::VOICE_COUNT {
        let id = controller.play_with(&tone, paused).unwrap();
        controller.set_protect(id, true).unwrap();
        if controller.queue_space() < 2 {
            render(&mut renderer, 0);
        }
    }
    let extra = controller.play(&tone).unwrap();
    for _ in 0..8 {
        render(&mut renderer, 0);
    }
    let all = events(&mut controller);
    assert_eq!(all.last(), Some(&Event::Failed(extra)));
    assert_eq!(controller.status().voices, crate::VOICE_COUNT as u32);
}
