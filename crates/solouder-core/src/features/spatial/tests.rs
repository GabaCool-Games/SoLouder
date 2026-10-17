//! Regression tests for 3D playback: #95, #240, reference patches 0013,
//! 0060 and 0061, and the behaviors port-status.md records.

use super::Attenuation;
use crate::features::playback::flags;
use crate::{Clipper, Handle, Mixer, MixerConfig, Pcm, PlayOptions, Sound};

/// Stereo at 44.1 kHz with hard clipping, so small samples come out scaled
/// by the post-clip scaler alone.
pub(super) fn mixer() -> Mixer {
    Mixer::new(MixerConfig {
        clipper: Clipper::Hard,
        ..MixerConfig::default()
    })
    .unwrap()
}

/// A mono sound of `frames` frames at `value`, attenuated by inverse distance.
pub(super) fn dc(frames: usize, value: f32) -> Sound {
    let mut sound = Sound::new(Pcm::from_planar(vec![value; frames], 1, 44100.0).unwrap());
    sound.set_3d_attenuation(Attenuation::InverseDistance, 1.0);
    sound
}

pub(super) fn render(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * mixer.config().channels as usize];
    mixer.mix_interleaved(&mut out);
    out
}

pub(super) const STILL: [f32; 3] = [0.0; 3];

pub(super) fn options() -> PlayOptions {
    PlayOptions {
        volume: Some(1.0),
        ..PlayOptions::default()
    }
}

fn has_flag(mixer: &Mixer, handle: Handle, flag: u32) -> bool {
    mixer.voices.get(handle).unwrap().has_flag(flag)
}

/// Each default stereo speaker hears a voice straight ahead at this gain.
fn ahead() -> f32 {
    (1.0 / 5.0_f32.sqrt() + 1.0) / 2.0
}

/// #95: the first mixed frame is already attenuated, not ramped down from
/// full volume. The attenuation applies twice, through the speaker gain and
/// the overall volume, as in SoLoud.
#[test]
fn play_3d_attenuates_the_first_frame() {
    let mut mixer = mixer();
    let h = mixer.play_3d(&dc(4096, 0.5), [0.0, 0.0, -4.0], STILL, options());
    assert_eq!(mixer.overall_volume(h), Some(0.25));
    let out = render(&mut mixer, 64);
    let expected = 0.5 * (0.25 * ahead()) * 0.25 * 0.95;
    // The linear resampler starts one frame late.
    for &sample in &out[4..64] {
        assert!((sample - expected).abs() < 1e-6, "{sample} vs {expected}");
    }
}

/// Reference patch 0013 (#361): a slot last used by a listener-relative voice
/// does not make the next 3D voice in it listener-relative.
#[test]
fn play_3d_in_a_reused_slot_uses_its_own_flags() {
    let mut mixer = mixer();
    mixer.set_3d_listener_position([100.0, 0.0, 0.0]);
    let mut relative = dc(4096, 0.5);
    relative.set_3d_listener_relative(true);
    let r = mixer.play_3d(&relative, [1.0, 0.0, 0.0], STILL, options());
    assert_eq!(mixer.overall_volume(r), Some(1.0));
    mixer.update_3d_audio();
    mixer.stop(r);
    let w = mixer.play_3d(&dc(4096, 0.5), [1.0, 0.0, 0.0], STILL, options());
    assert_eq!(w.raw() & 0xfff, r.raw() & 0xfff, "same slot");
    assert_eq!(mixer.overall_volume(w), Some(1.0 / 99.0));
}

/// #240: `play_3d` marks voices below 0.01 inaudible, `update_3d_audio` only
/// those below 0.001. Inaudible voices are not mixed.
#[test]
fn play_and_update_use_different_thresholds() {
    let mut mixer = mixer();
    // Inverse distance at 200 units: 1/200 = 0.005.
    let h = mixer.play_3d(&dc(44100, 0.5), [0.0, 0.0, -200.0], STILL, options());
    assert!(has_flag(&mixer, h, flags::INAUDIBLE));
    assert_eq!(mixer.active_voice_count(), 0);
    assert!(render(&mut mixer, 256).iter().all(|&s| s == 0.0));
    mixer.update_3d_audio();
    assert!(!has_flag(&mixer, h, flags::INAUDIBLE));
    assert!(render(&mut mixer, 256).iter().any(|&s| s != 0.0));
    mixer.set_3d_source_position(h, [0.0, 0.0, -2000.0]);
    mixer.update_3d_audio();
    assert!(has_flag(&mixer, h, flags::INAUDIBLE));
    assert!(mixer.is_valid_voice_handle(h), "inaudible, not stopped");
}

#[test]
fn inaudible_kill_stops_voices() {
    let mut mixer = mixer();
    let mut sound = dc(44100, 0.5);
    sound.set_inaudible_behavior(false, true);
    let far = mixer.play_3d(&sound, [0.0, 0.0, -200.0], STILL, options());
    assert!(!mixer.is_valid_voice_handle(far), "killed by play_3d");
    let near = mixer.play_3d(&sound, [0.0, 0.0, -2.0], STILL, options());
    mixer.set_3d_source_position(near, [0.0, 0.0, -2000.0]);
    assert!(mixer.is_valid_voice_handle(near));
    mixer.update_3d_audio();
    assert!(
        !mixer.is_valid_voice_handle(near),
        "killed by update_3d_audio"
    );
}

/// An inaudible voice that ticks keeps its place in time without being
/// heard.
#[test]
fn inaudible_ticking_voices_advance_silently() {
    let mut mixer = mixer();
    let mut sound = dc(44100, 0.5);
    sound.set_inaudible_behavior(true, false);
    let h = mixer.play_3d(&sound, [0.0, 0.0, -5000.0], STILL, options());
    assert!(render(&mut mixer, 1000).iter().all(|&s| s == 0.0));
    assert!(mixer.stream_position(h).unwrap() > 0.02);
    mixer.set_3d_source_position(h, [0.0, 0.0, -1.0]);
    mixer.update_3d_audio();
    assert!(render(&mut mixer, 256).iter().any(|&s| s != 0.0));
}

/// Reference patch 0060: an inaudible ticking voice whose loop gives no
/// audio (its loop point is past the end) no longer wraps forever.
#[test]
fn ticking_voice_with_an_empty_loop_ends_its_mix() {
    let mut mixer = mixer();
    let mut sound = dc(1000, 0.5);
    sound.set_looping(true);
    sound.set_loop_point(1.0);
    sound.set_inaudible_behavior(true, false);
    let h = mixer.play_3d(&sound, [0.0, 0.0, -5000.0], STILL, options());
    render(&mut mixer, 4096);
    assert!(mixer.is_valid_voice_handle(h));
}

/// Reference patch 0061: a ticking voice clears the rest of a short block,
/// so a voice made audible during its last block does not play the end of
/// the block read two blocks before.
#[test]
fn ticking_voice_clears_the_end_of_its_last_block() {
    let mut mixer = mixer();
    let mut sound = dc(1300, 0.5);
    sound.set_inaudible_behavior(true, false);
    let h = mixer.play_3d(&sound, [0.0, 0.0, -1.0], STILL, options());
    render(&mut mixer, 1000);
    // Inaudible while it reads its last block (frames 1024 to 1299), then
    // audible again for the rest.
    mixer.set_volume(h, 0.0);
    mixer.update_3d_audio();
    render(&mut mixer, 100);
    mixer.set_volume(h, 1.0);
    mixer.update_3d_audio();
    let out = render(&mut mixer, 400);
    // The linear resampler plays frame 1299 one frame late.
    let end = (1301 - 1100) * 2;
    assert!(out[end - 2] != 0.0, "the last frame is heard");
    assert!(
        out[end..].iter().all(|&s| s == 0.0),
        "{:?}",
        &out[end - 4..end + 8]
    );
}

#[test]
fn setters_change_only_3d_voices() {
    let mut mixer = mixer();
    let sound = dc(44100, 0.5);
    let plain = mixer.play_with(&sound, options());
    mixer.set_3d_source_position(plain, [0.0, 0.0, -100.0]);
    mixer.set_3d_source_attenuation(plain, Attenuation::LinearDistance, 1.0);
    mixer.update_3d_audio();
    assert_eq!(mixer.overall_volume(plain), Some(1.0));
    let spatial = mixer.play_3d(&sound, [0.0, 0.0, -1.0], STILL, options());
    mixer.set_3d_source_min_max_distance(spatial, 2.0, 100.0);
    mixer.set_3d_source_position(spatial, [0.0, 0.0, -8.0]);
    mixer.update_3d_audio();
    assert_eq!(mixer.overall_volume(spatial), Some(0.25));
}

#[test]
fn speaker_layouts_and_bounds() {
    let stereo = mixer();
    assert_eq!(stereo.speaker_position(0), Some([2.0, 0.0, 1.0]));
    assert_eq!(stereo.speaker_position(1), Some([-2.0, 0.0, 1.0]));
    assert_eq!(stereo.speaker_position(2), None);
    let mut surround = Mixer::new(MixerConfig {
        channels: 6,
        ..MixerConfig::default()
    })
    .unwrap();
    assert_eq!(surround.speaker_position(3), Some([0.0; 3]), "subwoofer");
    assert_eq!(surround.set_speaker_position(5, [1.0, 2.0, 3.0]), Ok(()));
    assert_eq!(surround.speaker_position(5), Some([1.0, 2.0, 3.0]));
    assert!(surround.set_speaker_position(6, [1.0, 0.0, 0.0]).is_err());
}

/// A zero speaker vector hears every direction at the attenuation alone.
#[test]
fn zero_speaker_hears_everything() {
    let mut mixer = mixer();
    mixer.set_speaker_position(0, [0.0; 3]).unwrap();
    let h = mixer.play_3d(&dc(4096, 0.5), [-3.0, 0.0, 0.0], STILL, options());
    let voice = mixer.voices.get(h).unwrap();
    assert_eq!(voice.channel_volume[0], 1.0 / 3.0);
}

/// A paused voice's gains jump to its new volume when updated, so it does
/// not ramp from the old volume when it resumes (#136, #238). As in SoLoud,
/// the jump uses the speaker gains from before the update.
#[test]
fn paused_voices_jump_to_their_new_volume() {
    let mut mixer = mixer();
    let paused = PlayOptions {
        paused: true,
        ..options()
    };
    let h = mixer.play_3d(&dc(4096, 0.5), [0.0, 0.0, -1.0], STILL, paused);
    mixer.set_3d_source_position(h, [0.0, 0.0, -2.0]);
    mixer.update_3d_audio();
    let voice = mixer.voices.get(h).unwrap();
    assert_eq!(voice.overall_volume, 0.5);
    assert_eq!(voice.current_channel_volume[0], ahead() * 0.5);
    assert_eq!(voice.channel_volume[0], 0.5 * ahead());
}
