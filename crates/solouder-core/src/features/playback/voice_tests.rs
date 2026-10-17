use super::*;
use crate::features::playback::Sound;

fn voice(channels: u32, frames: usize) -> Voice {
    let samples = (0..channels as usize * frames).map(|i| i as f32).collect();
    Sound::new(Pcm::from_planar(samples, channels, 44100.0).unwrap()).voice()
}

fn offset(v: &Voice) -> u32 {
    match v.source {
        VoiceSource::Pcm { offset, .. } => offset,
        _ => unreachable!(),
    }
}

#[test]
fn read_copies_planar_frames_until_the_end() {
    let mut v = voice(2, 3);
    let mut block = vec![0.0; BLOCK * MAX];
    assert_eq!(v.read(&mut block, 0, 2), 2);
    assert_eq!(v.read(&mut block, 2, 5), 1);
    assert_eq!(block[..3], [0.0, 1.0, 2.0]);
    assert_eq!(block[BLOCK..BLOCK + 3], [3.0, 4.0, 5.0]);
    assert!(v.has_ended(&Queues::default()));
    v.set_flag(flags::LOOPING, true);
    assert!(!v.has_ended(&Queues::default()));
}

#[test]
fn seek_rewinds_or_skips_at_the_base_rate() {
    let mut v = voice(1, 44100);
    v.set_relative_play_speed(2.0);
    v.seek(0.25);
    assert_eq!((offset(&v), v.stream_position), (11025, 0.25));
    v.seek(0.5);
    assert_eq!(offset(&v), 22050);
    v.seek(0.1);
    assert_eq!(offset(&v), 4410);
    v.seek(5.0);
    assert!(v.has_ended(&Queues::default()));
    assert_eq!(v.stream_position, 5.0);
    v.seek(f64::NAN);
    assert_eq!(offset(&v), 44100);
}

#[test]
fn pan_law_is_constant_power() {
    let mut v = voice(1, 1);
    v.set_pan(-1.0);
    assert_eq!(v.channel_volume[..2], [1.0, 0.0]);
    v.set_pan(0.3);
    let [l, r] = [v.channel_volume[0], v.channel_volume[1]];
    assert!((l * l + r * r - 1.0).abs() < 1e-6);
}

#[test]
fn rejects_non_positive_speeds() {
    let mut v = voice(1, 1);
    assert!(!v.set_relative_play_speed(0.0));
    assert!(!v.set_relative_play_speed(f32::NAN));
    assert!(v.set_relative_play_speed(2.0));
    assert_eq!(v.sample_rate, 88200.0);
}
