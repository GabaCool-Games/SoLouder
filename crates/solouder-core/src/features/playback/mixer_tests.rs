use super::*;
use crate::contracts::Pcm;
use crate::dsp::{Clipper, Resampler};
use crate::features::playback::{ConfigError, Handle, PlayOptions, Sound};

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

fn noise(frames: usize) -> Sound {
    let mut x = 1_u32;
    let samples = (0..frames)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as f32 / 8_388_608.0 - 1.0
        })
        .collect();
    Sound::new(Pcm::from_planar(samples, 1, 44100.0).unwrap())
}

fn render(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![f32::NAN; frames * mixer.config().channels as usize];
    mixer.mix_interleaved(&mut out);
    out
}

#[test]
fn new_rejects_invalid_config() {
    let config = MixerConfig {
        channels: 3,
        ..MixerConfig::default()
    };
    assert_eq!(
        Mixer::new(config).unwrap_err(),
        ConfigError::UnsupportedChannelCount(3)
    );
}

#[test]
fn mix_without_sources_is_silent() {
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    assert!(render(&mut mixer, 600).iter().all(|&s| s == 0.0));
    let mut large = vec![1.0_f32; 2 * 10_000 + 1];
    mixer.mix_interleaved(&mut large);
    assert!(large.iter().all(|&s| s == 0.0));
    mixer.mix_interleaved(&mut []);
}

#[test]
fn centered_mono_plays_at_equal_power_one_frame_late() {
    let mut mixer = mixer();
    mixer.play(&dc(0.5, 4096));
    let out = render(&mut mixer, 8);
    let gain = (std::f64::consts::FRAC_PI_4.cos()) as f32;
    assert_eq!(out[..2], [0.0, 0.0]);
    for frame in out[2..].chunks(2) {
        assert!((frame[0] - 0.5 * gain).abs() < 1e-6 && frame[0] == frame[1]);
    }
}

#[test]
fn split_mixes_match_one_mix() {
    for speed in [1.0, 0.7, 1.9] {
        let sound = noise(3000);
        let mut whole = mixer();
        let h = whole.play(&sound);
        whole.set_relative_play_speed(h, speed).unwrap();
        let expected = render(&mut whole, 2000);

        let mut split = mixer();
        let h = split.play(&sound);
        split.set_relative_play_speed(h, speed).unwrap();
        let mut actual = Vec::new();
        for frames in [1, 3, 5, 511, 513, 7, 960] {
            actual.extend(render(&mut split, frames));
        }
        assert_eq!(actual.len(), expected.len());
        for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
            assert!((a - e).abs() < 1e-6, "speed {speed} sample {i}: {a} vs {e}");
        }
    }
}

#[test]
fn voice_stops_after_its_last_block() {
    let mut mixer = mixer();
    let h = mixer.play(&dc(0.5, 600));
    render(&mut mixer, 512);
    assert!(mixer.is_valid_voice_handle(h));
    render(&mut mixer, 512);
    assert!(!mixer.is_valid_voice_handle(h));
    assert_eq!(mixer.voice_count(), 0);
    mixer.set_volume(h, 1.0);
    assert_eq!(mixer.volume(h), None);
}

#[test]
fn looping_voice_counts_loops_and_keeps_playing() {
    let mut mixer = mixer();
    let mut sound = dc(0.5, 300);
    sound.set_looping(true);
    let h = mixer.play(&sound);
    let out = render(&mut mixer, 4096);
    assert!(out[4..].iter().all(|&s| s > 0.3));
    assert_eq!(mixer.loop_count(h), Some(4096 / 300));
}

#[test]
fn paused_voice_is_silent_and_resumes() {
    let mut mixer = mixer();
    let options = PlayOptions {
        paused: true,
        ..PlayOptions::default()
    };
    let h = mixer.play_with(&dc(0.5, 4096), options);
    assert!(render(&mut mixer, 256).iter().all(|&s| s == 0.0));
    assert_eq!(mixer.voice_stream_time(h), Some(0.0));
    mixer.set_pause(h, false);
    assert!(render(&mut mixer, 256)[100] > 0.3);
}

#[test]
fn extreme_speed_renders_silence_instead_of_hanging() {
    let mut mixer = mixer();
    let mut sound = dc(0.5, 4410);
    sound.set_looping(true);
    let h = mixer.play(&sound);
    mixer.set_relative_play_speed(h, 600.0).unwrap();
    assert!(render(&mut mixer, 1024).iter().all(|&s| s == 0.0));
    assert!(mixer.is_valid_voice_handle(h));
}

#[test]
fn leftover_frames_continue_the_volume_ramp() {
    let mut mixer = mixer();
    let h = mixer.play_with(
        &dc(1.0, 4096),
        PlayOptions {
            volume: Some(0.0),
            ..PlayOptions::default()
        },
    );
    mixer.set_volume(h, 1.0);
    let out = render(&mut mixer, 6);
    let gain = (std::f64::consts::FRAC_PI_4.cos()) as f32;
    for j in 1..6 {
        assert!(
            (out[j * 2] - gain * (j + 1) as f32 / 6.0).abs() < 1e-5,
            "frame {j}"
        );
    }
}

#[test]
fn invalid_speed_and_voice_count_are_rejected() {
    let mut mixer = mixer();
    let h = mixer.play(&dc(0.5, 64));
    assert!(mixer.set_relative_play_speed(h, 0.0).is_err());
    assert!(mixer.set_relative_play_speed(Handle::INVALID, 0.0).is_ok());
    assert!(mixer.set_max_active_voice_count(0).is_err());
    assert!(mixer.set_max_active_voice_count(1024).is_err());
    assert!(mixer.set_max_active_voice_count(300).is_ok());
}

#[test]
fn more_than_255_voices_can_be_active() {
    let mut mixer = mixer();
    mixer.set_max_active_voice_count(300).unwrap();
    let mut sound = dc(0.001, 4410);
    sound.set_looping(true);
    for _ in 0..300 {
        mixer.play(&sound);
    }
    render(&mut mixer, 1024);
    assert_eq!(mixer.active_voice_count(), 300);
}

fn audible_frames(out: &[f32]) -> usize {
    out.chunks(2).filter(|frame| frame[0] != 0.0).count()
}

#[test]
fn sound_tails_play_out_when_mixes_end_mid_block() {
    // Reference patch 0024: the last block used to be dropped when a mix
    // ended partway through it.
    let sound = dc(0.5, 1000);
    for resampler in [Resampler::Point, Resampler::Linear, Resampler::CatmullRom] {
        for speed in [1.0, 0.73] {
            let mut whole = mixer();
            whole.set_resampler(resampler);
            let handle = whole.play(&sound);
            whole.set_relative_play_speed(handle, speed).unwrap();
            let mut expected = vec![0.0; 2 * 4096];
            whole.mix_interleaved(&mut expected);

            for chunk in [256, 441, 480] {
                let mut parts = mixer();
                parts.set_resampler(resampler);
                let handle = parts.play(&sound);
                parts.set_relative_play_speed(handle, speed).unwrap();
                let mut out = vec![0.0; 2 * 4096];
                for part in out.chunks_exact_mut(2 * chunk) {
                    parts.mix_interleaved(part);
                }
                assert_eq!(
                    audible_frames(&out),
                    audible_frames(&expected),
                    "{resampler:?} at speed {speed} in {chunk}-frame mixes"
                );
                assert!(!parts.is_valid_voice_handle(handle));
            }
        }
    }
}

#[test]
fn a_clocked_play_keeps_its_delay_and_the_requested_pause() {
    let mut mixer = mixer();
    // Point sampling has no interpolation lag, so voices start on their frame.
    mixer.set_resampler(Resampler::Point);
    let first = mixer.play_clocked(5.0, &dc(0.5, 4096), PlayOptions::default());
    let paused = PlayOptions {
        paused: true,
        ..PlayOptions::default()
    };
    let second = mixer.play_clocked(5.001, &dc(0.25, 4096), paused);
    assert_eq!(mixer.is_paused(first), Some(false));
    assert_eq!(mixer.is_paused(second), Some(true));
    let first_only = render(&mut mixer, 64)[0];
    assert!(first_only > 0.0);
    mixer.set_pause(second, false);
    let out = render(&mut mixer, 64);
    // 0.001 s at 44.1 kHz: 44 frames of delay, then both voices.
    assert_eq!(out[43 * 2], first_only);
    assert!((out[44 * 2] - first_only * 1.5).abs() < 1e-6);
}

#[test]
fn ticking_voices_over_the_active_cap_still_get_resample_blocks() {
    let mut mixer = mixer();
    mixer.set_max_active_voice_count(2).unwrap();
    let mut ticking = dc(0.25, 4410);
    ticking.set_looping(true);
    ticking.set_inaudible_behavior(true, false);
    for _ in 0..3 {
        mixer.play(&ticking);
    }
    mixer.play(&dc(0.25, 4410));
    let out = render(&mut mixer, 4096);
    assert!(out.iter().all(|s| s.is_finite()));
    assert_eq!(mixer.active_voice_count(), 2);
    assert!(out.iter().any(|&s| s != 0.0));
}

/// #314, patch 0052: a resampled voice skipped up to one step of source per
/// block and ran about 0.09% fast at 44.1 -> 48 kHz.
#[test]
fn resampled_playhead_does_not_drift() {
    let frames = 44100 * 20;
    let ramp = (0..frames).map(|i| i as f32 / (1 << 21) as f32).collect();
    let sound = Sound::new(Pcm::from_planar(ramp, 1, 44100.0).unwrap());
    for chunk in [512, 441] {
        let mut mixer = Mixer::new(MixerConfig {
            sample_rate: 48000,
            channels: 1,
            buffer_size: 512,
            clipper: Clipper::Hard,
            ..MixerConfig::default()
        })
        .unwrap();
        mixer.set_resampler(Resampler::Point);
        mixer.play(&sound);
        let mut out = vec![0.0_f32; 48000 * 19];
        for part in out.chunks_mut(chunk) {
            mixer.mix_interleaved(part);
        }
        let step = (44100.0_f32 / 48000.0 * (1 << 20) as f32).floor() as u64;
        let gain = f64::from(out[100]) / ((100 * step) >> 20) as f64;
        for (n, &s) in out.iter().enumerate().skip(100) {
            let played = (f64::from(s) / gain).round() as u64;
            assert_eq!(
                played,
                (n as u64 * step) >> 20,
                "frame {n}, {chunk}-frame mixes"
            );
        }
    }
}
