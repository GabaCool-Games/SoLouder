use super::*;
use crate::contracts::Pcm;
use crate::dsp::Clipper;
use crate::features::playback::{Handle, Mixer, PlayOptions};
use crate::features::playback::{MixerConfig, Sound};

fn mixer() -> Mixer {
    let config = MixerConfig {
        clipper: Clipper::Hard,
        ..MixerConfig::default()
    };
    let mut mixer = Mixer::new(config).unwrap();
    mixer.set_post_clip_scaler(1.0);
    mixer
}

fn dc(value: f32, frames: usize) -> Sound {
    Sound::new(Pcm::from_planar(vec![value; frames], 1, 44100.0).unwrap())
}

fn render(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![f32::NAN; frames * mixer.config().channels as usize];
    mixer.mix_interleaved(&mut out);
    out
}

#[test]
fn bus_mixes_its_voices_through_its_own_pan() {
    let mut mixer = mixer();
    let bus = Bus::new();
    mixer.play(&bus);
    assert_ne!(
        mixer.bus_play(&bus, &dc(0.5, 4096), PlayOptions::default()),
        Handle::INVALID
    );
    let out = render(&mut mixer, 16);
    // Two resampling stages, each a frame late, and two equal-power pans.
    assert_eq!(out[..4], [0.0; 4]);
    for frame in out[4..].chunks(2) {
        assert!((frame[0] - 0.25).abs() < 1e-6 && frame[0] == frame[1]);
    }
}

#[test]
fn bus_is_silent_until_its_handle_is_looked_up() {
    let mut mixer = mixer();
    let bus = Bus::new();
    let bus_handle = mixer.play(&bus);
    let options = PlayOptions {
        bus: bus_handle,
        ..PlayOptions::default()
    };
    let child = mixer.play_with(&dc(0.5, 4096), options);
    assert!(render(&mut mixer, 64).iter().all(|&s| s == 0.0));
    assert_eq!(
        mixer.stream_position(child),
        Some(f64::from(64.0_f32 / 44100.0))
    );
    // Stopping a bus that never looked up its handle leaves its voices.
    mixer.stop(bus_handle);
    assert_eq!(mixer.voice_count(), 1);

    let bus_handle = mixer.play(&bus);
    mixer.annex_sound(&bus, child);
    assert_eq!(mixer.bus_voice_count(&bus), 1);
    assert!(render(&mut mixer, 64)[8..].iter().all(|&s| s > 0.0));
    mixer.stop(bus_handle);
    assert_eq!(mixer.voice_count(), 0);
}

#[test]
fn playing_a_bus_again_stops_its_voice_and_children() {
    let mut mixer = mixer();
    let bus = Bus::new();
    let first = mixer.play(&bus);
    mixer.bus_play(&bus, &dc(0.5, 4096), PlayOptions::default());
    let second = mixer.play(&bus.clone());
    assert_ne!(first, second);
    assert!(!mixer.is_valid_voice_handle(first));
    assert_eq!(mixer.voice_count(), 1);
    assert_eq!(mixer.bus_voice_count(&bus), 0);
}

#[test]
fn bus_calls_need_a_playing_bus() {
    let mut mixer = mixer();
    let mut bus = Bus::new();
    let sound = dc(0.5, 64);
    assert_eq!(
        mixer.bus_play(&bus, &sound, PlayOptions::default()),
        Handle::INVALID
    );
    assert_eq!(mixer.bus_voice_count(&bus), 0);
    assert_eq!(mixer.bus_wave(&bus), None);
    assert_eq!(mixer.bus_fft(&bus), None);
    mixer.set_bus_visualization(&mut bus, true);
    assert!(bus.visualization());
    assert_eq!(mixer.voice_count(), 0);
}

#[test]
fn set_channels_takes_speaker_layouts_only() {
    let mut bus = Bus::new();
    for channels in [0, 3, 5, 7, 9] {
        assert_eq!(
            bus.set_channels(channels),
            Err(ControlError::InvalidChannels)
        );
    }
    assert_eq!(bus.set_channels(6), Ok(()));
    assert_eq!(bus.channels(), 6);
}

#[test]
fn visualization_records_peaks_and_the_summed_wave() {
    let mut mixer = mixer();
    let mut bus = Bus::new();
    bus.set_visualization(true);
    mixer.play(&bus);
    let options = PlayOptions {
        pan: -1.0,
        ..PlayOptions::default()
    };
    mixer.bus_play(&bus, &dc(0.5, 4096), options);
    render(&mut mixer, 1024);
    let wave = mixer.bus_wave(&bus).unwrap();
    assert!(wave[8..].iter().all(|&s| s == 0.5));
    assert_eq!(mixer.bus_approximate_volume(&bus, 0), 0.5);
    assert_eq!(mixer.bus_approximate_volume(&bus, 1), 0.0);
    assert_eq!(mixer.bus_approximate_volume(&bus, 2), 0.0);
    let fft = mixer.bus_fft(&bus).unwrap();
    assert!(fft[0] > 100.0);
}

#[test]
fn bus_seeks_forward_only_by_mixing_its_voices() {
    let mut mixer = mixer();
    let bus = Bus::new();
    let bus_handle = mixer.play(&bus);
    let child = mixer.bus_play(&bus, &dc(0.5, 30_000), PlayOptions::default());
    render(&mut mixer, 512);
    assert_eq!(mixer.seek(bus_handle, 0.5), Ok(()));
    assert_eq!(mixer.stream_position(bus_handle), Some(0.5));
    // The voice was mixed over the 21538 frames skipped and has ended.
    render(&mut mixer, 8192);
    assert!(!mixer.is_valid_voice_handle(child));

    let position = mixer.stream_position(bus_handle);
    assert_eq!(
        mixer.seek(bus_handle, 0.1),
        Err(ControlError::CannotSeekBack)
    );
    assert_eq!(mixer.stream_position(bus_handle), position);
}

#[test]
fn bus_seek_skips_voices_played_since_the_last_mix() {
    let mut mixer = mixer();
    let bus = Bus::new();
    let bus_handle = mixer.play(&bus);
    let sound = dc(0.5, 30_000);
    let first = mixer.bus_play(&bus, &sound, PlayOptions::default());
    render(&mut mixer, 512);
    mixer.stop(first);
    let second = mixer.bus_play(&bus, &sound, PlayOptions::default());
    assert_eq!(first.slot(), second.slot());
    assert_eq!(mixer.seek(bus_handle, 0.2), Ok(()));
    assert!(render(&mut mixer, 512).iter().all(|s| s.is_finite()));
    assert!(mixer.is_valid_voice_handle(second));
}
