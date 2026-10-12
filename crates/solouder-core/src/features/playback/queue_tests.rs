use super::*;
use crate::contracts::Pcm;
use crate::dsp::Clipper;
use crate::features::playback::Mixer;
use crate::features::playback::{MixerConfig, QueueError};

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
fn queued_sounds_play_back_to_back() {
    let mut mixer = mixer();
    let queue = Queue::new();
    let handle = mixer.play(&queue);
    let (first, second) = (dc(0.5, 300), dc(0.25, 300));
    assert_eq!(mixer.queue_play(&queue, &first), Ok(()));
    assert_eq!(mixer.queue_play(&queue, &second), Ok(()));
    assert!(mixer.queue_is_current(&queue, &first));
    let out = render(&mut mixer, 600);
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    // Linear resampling delays the voice by one frame.
    let pan = std::f32::consts::FRAC_1_SQRT_2;
    assert!(left[1..300].iter().all(|&s| (s - 0.5 * pan).abs() < 1e-6));
    assert!(left[301..].iter().all(|&s| (s - 0.25 * pan).abs() < 1e-6));
    assert_eq!(mixer.queue_count(&queue), 0);
    assert!(!mixer.queue_is_current(&queue, &second));
    render(&mut mixer, 512);
    assert!(!mixer.is_valid_voice_handle(handle));
}

#[test]
fn queue_play_needs_a_playing_queue_with_room() {
    let mut mixer = mixer();
    let queue = Queue::new();
    let sound = dc(0.5, 64);
    assert_eq!(
        mixer.queue_play(&queue, &sound),
        Err(QueueError::NotPlaying)
    );
    mixer.play(&queue);
    for _ in 0..QUEUE_MAX {
        assert_eq!(mixer.queue_play(&queue, &sound), Ok(()));
    }
    assert_eq!(mixer.queue_play(&queue, &sound), Err(QueueError::Full));
    assert_eq!(mixer.queue_count(&queue), QUEUE_MAX);
}

#[test]
fn queued_sounds_wait_while_the_queue_is_stopped() {
    let mut mixer = mixer();
    let queue = Queue::new();
    let handle = mixer.play(&queue);
    let sound = dc(0.5, 4096);
    mixer.queue_play(&queue, &sound).unwrap();
    mixer.stop(handle);
    assert_eq!(mixer.queue_count(&queue), 1);
    assert!(!mixer.queue_is_current(&queue, &sound));
    mixer.play(&queue);
    assert!(mixer.queue_is_current(&queue, &sound));
    let out = render(&mut mixer, 16);
    assert!(out[2..].iter().all(|&s| s > 0.0));
}

#[test]
fn queue_seeks_forward_only() {
    let mut mixer = mixer();
    let queue = Queue::new();
    let handle = mixer.play(&queue);
    mixer.queue_play(&queue, &dc(0.5, 1000)).unwrap();
    mixer.queue_play(&queue, &dc(0.25, 1000)).unwrap();
    assert_eq!(mixer.seek(handle, 1500.0 / 44100.0), Ok(()));
    assert_eq!(mixer.queue_count(&queue), 1);
    assert_eq!(
        mixer.seek(handle, 0.0),
        Err(crate::features::playback::ControlError::CannotSeekBack)
    );
}

#[test]
fn set_params_takes_one_to_eight_channels() {
    let mut queue = Queue::new();
    assert!(queue.set_params(48000.0, 0).is_err());
    assert!(queue.set_params(48000.0, 9).is_err());
    assert_eq!(queue.set_params(48000.0, 8), Ok(()));
    assert_eq!((queue.channels(), queue.sample_rate()), (8, 48000.0));
    let stereo = Sound::new(Pcm::from_planar(vec![0.0; 8], 2, 22050.0).unwrap());
    queue.set_params_from(&stereo);
    assert_eq!((queue.channels(), queue.sample_rate()), (2, 22050.0));
}
