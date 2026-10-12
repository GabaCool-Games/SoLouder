//! Output lifecycle on the real default device: repeated open and close,
//! drop while playing, two outputs at once, and init failures.
//!
//! Without an audio device the device tests are skipped; set
//! `SOLOUDER_REQUIRE_DEVICE=1` to make a missing device a failure.

use std::thread::sleep;
use std::time::{Duration, Instant};

use cpal::ErrorKind;
use solouder_core::{Controller, Event, FinishReason, Pcm, Sound};
use solouder_cpal::{
    MAX_BUFFER_FRAMES, Output, OutputConfig, OutputError, OutputState, StreamPreferences, open,
};

/// Errors that mean "this machine has no usable output", not a bug.
fn no_device(error: &OutputError) -> bool {
    match error {
        OutputError::NoDevice => true,
        OutputError::Backend(error) => matches!(
            error.kind(),
            ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable | ErrorKind::DeviceBusy
        ),
        _ => false,
    }
}

fn open_device(config: &OutputConfig) -> Option<(Output, Controller)> {
    match open(config) {
        Ok(opened) => Some(opened),
        Err(error) if no_device(&error) => {
            assert!(
                std::env::var_os("SOLOUDER_REQUIRE_DEVICE").is_none(),
                "no usable output device: {error}"
            );
            eprintln!("skipped: {error}");
            None
        }
        Err(error) => panic!("open failed: {error}"),
    }
}

fn blip(seconds: f32) -> Sound {
    let rate = 48_000.0;
    let samples = (0..(rate * seconds) as usize)
        .map(|i| (i as f32 * 0.05).sin() * 0.05)
        .collect();
    Sound::new(Pcm::from_planar(samples, 1, rate).unwrap())
}

fn wait_for_final(controller: &mut Controller, timeout: Duration) -> Option<Event> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        while let Some(event) = controller.poll_event() {
            if event.is_final() {
                return Some(event);
            }
        }
        sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn invalid_voice_count_fails_before_touching_the_device() {
    for count in [0, 4096, u32::MAX] {
        let config = OutputConfig {
            max_active_voices: count,
            ..OutputConfig::default()
        };
        assert!(matches!(open(&config), Err(OutputError::InvalidMaxActiveVoices(c)) if c == count));
    }
}

#[test]
fn invalid_buffer_sizes_fail_before_touching_the_device() {
    for frames in [0, MAX_BUFFER_FRAMES + 1, u32::MAX] {
        let config = OutputConfig {
            buffer_frames: Some(frames),
            ..OutputConfig::default()
        };
        assert!(matches!(open(&config), Err(OutputError::InvalidBufferFrames(f)) if f == frames));
    }
}

#[test]
fn unsupported_channel_preference_is_rejected() {
    let config = OutputConfig {
        stream: StreamPreferences {
            channels: Some(3),
            ..StreamPreferences::default()
        },
        ..OutputConfig::default()
    };
    match open(&config) {
        Err(OutputError::UnsupportedDevice) => {}
        Err(error) if no_device(&error) => {}
        Err(error) => panic!("unexpected error: {error}"),
        Ok(_) => panic!("a 3-channel mixer was opened"),
    }
}

#[test]
fn repeated_open_play_close() {
    let sound = blip(0.05);
    for _ in 0..5 {
        let Some((output, mut controller)) = open_device(&OutputConfig::default()) else {
            return;
        };
        let id = controller.play(&sound).unwrap();
        let event = wait_for_final(&mut controller, Duration::from_secs(5));
        assert_eq!(event, Some(Event::Finished(id, FinishReason::Ended)));
        assert_eq!(output.state(), OutputState::Playing);
        assert!(output.callback_count() > 0);
        assert!(output.callback_frames() > 0);
        assert!(!output.info().backend.is_empty());
        assert_eq!(controller.voice_ids_in_use(), 0);
        let renderer = output.close();
        assert!(
            renderer.is_some(),
            "the renderer must come back once callbacks stop"
        );
    }
}

#[test]
fn dropping_while_playing_stops_cleanly() {
    let sound = blip(10.0);
    for _ in 0..3 {
        let Some((output, mut controller)) = open_device(&OutputConfig::default()) else {
            return;
        };
        controller.play(&sound).unwrap();
        controller.play(&sound).unwrap();
        sleep(Duration::from_millis(50));
        let started = Instant::now();
        drop(output);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "drop waited too long"
        );
        // The controller outlives the output: controls neither panic nor
        // block, and stop being delivered once the queue fills.
        for _ in 0..10_000 {
            let _ = controller.set_global_volume(0.5);
        }
        while controller.poll_event().is_some() {}
    }
}

#[test]
fn two_outputs_run_independently() {
    let Some((first, mut a)) = open_device(&OutputConfig::default()) else {
        return;
    };
    let Some((second, mut b)) = open_device(&OutputConfig::default()) else {
        return;
    };
    let long = blip(10.0);
    let short = blip(0.05);
    a.play(&long).unwrap();
    let id = b.play(&short).unwrap();
    assert_eq!(
        wait_for_final(&mut b, Duration::from_secs(5)),
        Some(Event::Finished(id, FinishReason::Ended))
    );
    assert!(second.close().is_some());
    assert_eq!(first.state(), OutputState::Playing);
    let renders = a.status().renders;
    let deadline = Instant::now() + Duration::from_secs(5);
    while a.status().renders <= renders {
        assert!(
            Instant::now() < deadline,
            "closing one output stopped the other"
        );
        sleep(Duration::from_millis(5));
    }
    assert!(first.close().is_some());
}
