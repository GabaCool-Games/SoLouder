//! Regression tests for Doppler, distance delay, clocked and bus 3D plays,
//! colliders, attenuators and `LEFT_HANDED_3D`: #97, #98 and reference
//! patches 0062 to 0065.

use std::sync::{Arc, Mutex};

use super::tests::{STILL, dc, mixer, options, render};
use super::{Attenuation, Attenuator, Collider, SpatialVoice};
use crate::{Bus, ControlError, Handle, Mixer, MixerConfig, Pcm, Sound};

/// Seconds of one 512-frame mix at 44.1 kHz, as the mixer counts them.
fn block_seconds() -> f64 {
    f64::from(512.0_f32 / 44100.0)
}

/// The play speed a voice had over one 512-frame mix, from how far its
/// stream position moved.
fn speed_over_a_mix(mixer: &mut Mixer, handle: Handle) -> f64 {
    let before = mixer.stream_position(handle).unwrap();
    render(mixer, 512);
    (mixer.stream_position(handle).unwrap() - before) / block_seconds()
}

fn assert_near(actual: f64, expected: f32) {
    assert!(
        (actual - f64::from(expected)).abs() < 1e-6,
        "{actual} != {expected}"
    );
}

/// Patch 0062: approaching sources and listeners raise the pitch.
#[test]
fn doppler_follows_openal() {
    let mut mixer = mixer();
    let sound = dc(44100, 0.25);
    let up = mixer.play_3d(&sound, [0.0, 0.0, -10.0], [0.0, 0.0, 100.0], options());
    let down = mixer.play_3d(&sound, [0.0, 0.0, -10.0], [0.0, 0.0, -100.0], options());
    let before = mixer.stream_position(down).unwrap();
    assert_near(speed_over_a_mix(&mut mixer, up), 343.3 / (343.3 - 100.0));
    let receding = (mixer.stream_position(down).unwrap() - before) / block_seconds();
    assert_near(receding, 343.3 / (343.3 + 100.0));

    let mut mixer = super::tests::mixer();
    mixer.set_3d_listener_velocity([0.0, 0.0, -100.0]);
    let still = mixer.play_3d(&sound, [0.0, 0.0, -10.0], STILL, options());
    assert_near(speed_over_a_mix(&mut mixer, still), (343.3 + 100.0) / 343.3);
}

/// #98: the play speed and Doppler multiply, and the set speed reads back
/// without Doppler.
#[test]
fn doppler_composes_with_play_speed() {
    let mut mixer = mixer();
    let h = mixer.play_3d(
        &dc(44100, 0.25),
        [0.0, 0.0, -10.0],
        [0.0, 0.0, 100.0],
        options(),
    );
    mixer.set_relative_play_speed(h, 1.5).unwrap();
    assert_eq!(mixer.relative_play_speed(h), Some(1.5));
    let doppler = 343.3_f32 / (343.3 - 100.0);
    assert_near(speed_over_a_mix(&mut mixer, h), doppler * 1.5);
    mixer.set_3d_source_velocity(h, STILL);
    mixer.update_3d_audio();
    assert_near(speed_over_a_mix(&mut mixer, h), 1.5);
}

/// #97 and patch 0063: at or past the speed of sound the voice is held in
/// place, silent, without an infinite or NaN position, and plays on once
/// slower.
#[test]
fn the_speed_of_sound_holds_the_voice() {
    let mut mixer = mixer();
    let sonic = mixer.play_3d(
        &dc(44100, 0.25),
        [0.0, 0.0, -10.0],
        [0.0, 0.0, 400.0],
        options(),
    );
    let out = render(&mut mixer, 2048);
    assert_eq!(mixer.stream_position(sonic), Some(0.0));
    assert!(out.iter().all(|&s| s == 0.0));
    assert!(mixer.is_valid_voice_handle(sonic));
    mixer.set_3d_source_velocity(sonic, STILL);
    mixer.update_3d_audio();
    assert_near(speed_over_a_mix(&mut mixer, sonic), 1.0);
}

/// Patch 0064: the speed of sound must be finite and above 0.
#[test]
fn sound_speed_rejects_nan_and_infinity() {
    let mut mixer = mixer();
    for speed in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
        assert_eq!(
            mixer.set_3d_sound_speed(speed),
            Err(ControlError::InvalidSoundSpeed)
        );
    }
    assert_eq!(mixer.sound_speed_3d(), 343.3);
    assert_eq!(mixer.set_3d_sound_speed(1497.0), Ok(()));
    assert_eq!(mixer.sound_speed_3d(), 1497.0);
}

/// The first output frame that is not silent.
fn first_heard(out: &[f32]) -> usize {
    out.iter().position(|&s| s != 0.0).unwrap_or(out.len()) / 2
}

/// Patch 0065: `play_3d_clocked` delays by the distance from the listener,
/// as `play_3d` does; a listener-relative source by its own offset.
#[test]
fn distance_delay_counts_from_the_listener() {
    let mut sound = Sound::new(Pcm::from_planar(vec![0.25; 44100], 1, 44100.0).unwrap());
    sound.set_3d_distance_delay(true);
    let heard = |clocked: bool, sound: &Sound, listener: [f32; 3], position: [f32; 3]| {
        let mut mixer = mixer();
        mixer.set_3d_listener_position(listener);
        if clocked {
            mixer.play_3d_clocked(1.0, sound, position, STILL, options());
        } else {
            mixer.play_3d(sound, position, STILL, options());
        }
        first_heard(&render(&mut mixer, 8192))
    };
    // 34.33 units at 343.3 per second: 4410 frames, heard one frame later
    // through the linear resampler.
    let played = heard(false, &sound, [1000.0, 0.0, 0.0], [1000.0, 0.0, -34.33]);
    assert!((4409..=4412).contains(&played), "{played}");
    let clocked = heard(true, &sound, [1000.0, 0.0, 0.0], [1000.0, 0.0, -34.33]);
    assert_eq!(clocked, played);
    sound.set_3d_listener_relative(true);
    let relative = heard(true, &sound, [-1000.0, 0.0, 0.0], [0.0, 0.0, -34.33]);
    assert_eq!(relative, played);
    // Without distance delay the voice starts at once.
    sound.set_3d_distance_delay(false);
    assert!(heard(false, &sound, [0.0; 3], [0.0, 0.0, -34.33]) < 2);
}

#[test]
fn bus_plays_need_a_playing_bus() {
    let mut mixer = mixer();
    let bus = Bus::new();
    let sound = dc(4096, 0.5);
    let pos = [1.0, 0.0, -1.0];
    assert_eq!(
        mixer.bus_play_3d(&bus, &sound, pos, STILL, options()),
        Handle::INVALID
    );
    let at = mixer.bus_play_3d_clocked(&bus, 1.0, &sound, pos, STILL, options());
    assert_eq!(at, Handle::INVALID);
    mixer.play(&bus);
    let h = mixer.bus_play_3d(&bus, &sound, pos, STILL, options());
    assert!(mixer.is_valid_voice_handle(h));
}

/// Records what it was called with and returns a fixed factor.
#[derive(Debug, Default)]
struct Recorder {
    seen: Mutex<Vec<(SpatialVoice, i32)>>,
}

impl Collider for Recorder {
    fn collide(&self, voice: &SpatialVoice, user_data: i32) -> f32 {
        self.seen.lock().unwrap().push((*voice, user_data));
        0.5
    }
}

#[derive(Debug)]
struct Halving;

impl Attenuator for Halving {
    fn attenuate(&self, distance: f32, _min: f32, max: f32, rolloff: f32) -> f32 {
        if distance < max { 0.5 * rolloff } else { 0.0 }
    }
}

/// A collider sees the voice and its user data; its factor multiplies an
/// attenuator's, which replaces the attenuation model.
#[test]
fn colliders_and_attenuators() {
    let mut mixer = mixer();
    let recorder = Arc::new(Recorder::default());
    let mut sound = dc(4096, 0.5);
    sound.set_3d_collider(Some(recorder.clone()), -7);
    sound.set_3d_attenuator(Some(Arc::new(Halving)));
    sound.set_3d_min_max_distance(1.0, 10.0);
    let h = mixer.play_3d(&sound, [3.0, 0.0, -4.0], [1.0, 2.0, 3.0], options());
    assert_eq!(mixer.overall_volume(h), Some(0.25));
    {
        let seen = recorder.seen.lock().unwrap();
        let (voice, user_data) = seen[0];
        assert_eq!(user_data, -7);
        assert_eq!(voice.handle, h);
        assert_eq!(voice.position, [3.0, 0.0, -4.0]);
        assert_eq!(voice.velocity, [1.0, 2.0, 3.0]);
        assert_eq!((voice.min_distance, voice.max_distance), (1.0, 10.0));
    }
    // The model set per voice does not apply while there is an attenuator.
    mixer.set_3d_source_attenuation(h, Attenuation::LinearDistance, 0.5);
    mixer.update_3d_audio();
    assert_eq!(mixer.overall_volume(h), Some(0.125));
    mixer.set_3d_source_position(h, [0.0, 0.0, -20.0]);
    mixer.update_3d_audio();
    assert_eq!(mixer.overall_volume(h), Some(0.0));
    assert_eq!(recorder.seen.lock().unwrap().len(), 3);
    // Removing them from the source changes only later voices.
    sound.set_3d_collider(None, 0);
    sound.set_3d_attenuator(None);
    assert!(sound.collider_3d().is_none() && sound.attenuator_3d().is_none());
    let plain = mixer.play_3d(&sound, [0.0, 0.0, -4.0], STILL, options());
    assert_eq!(mixer.overall_volume(plain), Some(0.25));
    assert_eq!(recorder.seen.lock().unwrap().len(), 3);
}

/// `LEFT_HANDED_3D` renders a scene exactly as the right-handed mixer
/// renders it with every z negated, velocities included (see the #94 and
/// #105 entry in PORTING.md).
#[test]
fn left_handed_mirrors_z() {
    let render_scene = |left_handed: bool| {
        let z = if left_handed { 1.0 } else { -1.0 };
        let mut mixer = Mixer::new(MixerConfig {
            channels: 6,
            left_handed_3d: left_handed,
            ..MixerConfig::default()
        })
        .unwrap();
        let noise: Vec<f32> = (0..20000)
            .map(|i| ((i * 7919 % 200) as f32 - 100.0) / 200.0)
            .collect();
        let sound = Sound::new(Pcm::from_planar(noise, 1, 44100.0).unwrap());
        mixer.set_3d_listener_parameters(
            [1.0, 0.5, 2.0 * z],
            [0.3, 0.0, z],
            [0.1, 1.0, 0.0],
            [0.0, 0.0, 3.0 * z],
        );
        mixer.play_3d(
            &sound,
            [4.0, 0.0, 5.0 * z],
            [0.0, 0.0, -20.0 * z],
            options(),
        );
        mixer.play_3d(&sound, [-2.0, 1.0, -z], STILL, options());
        let mut out = render(&mut mixer, 700);
        mixer.set_3d_listener_at([-1.0, 0.0, 0.5 * z]);
        mixer.update_3d_audio();
        out.extend(render(&mut mixer, 700));
        out
    };
    let left = render_scene(true);
    assert!(left.iter().any(|&s| s != 0.0));
    assert_eq!(left, render_scene(false));
}

/// A distance delay of `i32::MAX` frames or more becomes `i32::MIN`, as x86
/// converts it in SoLoud; the voice's unsigned delay then holds it back 2^31
/// frames (about 13.5 hours), rather than starting it at once.
#[test]
fn a_distance_delay_past_i32_waits_2_pow_31_frames() {
    let mut mixer = mixer();
    assert!(mixer.set_3d_sound_speed(1.0e-6).is_ok());
    let mut sound = dc(64, 0.5);
    sound.set_3d_distance_delay(true);
    let h = mixer.play_3d(&sound, [0.0, 0.0, -1000.0], STILL, options());
    assert_eq!(mixer.voices.get(h).unwrap().delay_samples, 1 << 31);
}
