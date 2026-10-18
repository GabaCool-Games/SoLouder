//! Where the controller's 3D work happens and what it never touches:
//! colliders run on the game thread, results reach only the voice they were
//! computed for, and updates in flight are bounded.

use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};

use super::spatial_tests::{STILL, tone};
use super::*;
use crate::spatial::{Attenuation, Collider, SpatialVoice};
use crate::{Handle, Mixer, MixerConfig, PlayOptions};

fn render(renderer: &mut Renderer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    renderer.render(&mut out);
    out
}

fn events(controller: &mut Controller) -> Vec<Event> {
    std::iter::from_fn(|| controller.poll_event()).collect()
}

fn realtime(config: RealtimeConfig) -> (Controller, Renderer) {
    split(Mixer::new(MixerConfig::default()).unwrap(), config)
}

/// Records the thread and voice of every call.
#[derive(Debug, Default)]
struct Recorder(Mutex<Vec<(ThreadId, Handle, Option<VoiceId>)>>);

impl Collider for Recorder {
    fn collide(&self, voice: &SpatialVoice, _user_data: i32) -> f32 {
        let call = (thread::current().id(), voice.handle, voice.voice);
        self.0.lock().unwrap().push(call);
        0.5
    }
}

/// The renderer runs on another thread; the collider runs only on this one,
/// once per voice at the play and once per update, and sees the voice's ID.
#[test]
fn colliders_run_on_the_game_thread() {
    let recorder = Arc::new(Recorder::default());
    let mut sound = tone(20000, 1, 44100.0, 41);
    sound.set_3d_collider(Some(Arc::clone(&recorder) as Arc<dyn Collider>), 0);
    let (mut c, renderer) = realtime(RealtimeConfig::default());
    let renderer = Arc::new(Mutex::new(renderer));
    let audio = |frames| {
        let renderer = Arc::clone(&renderer);
        thread::spawn(move || render(&mut renderer.lock().unwrap(), frames))
            .join()
            .unwrap()
    };
    let id = c
        .play_3d(&sound, [1.0, 0.0, -1.0], STILL, PlayOptions::default())
        .unwrap();
    assert!(audio(256).iter().any(|&s| s != 0.0));
    for x in [2.0, -3.0] {
        c.set_3d_source_position(id, [x, 0.0, -1.0]);
        c.update_3d_audio().unwrap();
        audio(256);
    }
    let calls = recorder.0.lock().unwrap().clone();
    assert_eq!(calls.len(), 3);
    for (thread, handle, voice) in calls {
        assert_eq!(thread, thread::current().id());
        assert_eq!(handle, Handle::INVALID);
        assert_eq!(voice, Some(id));
    }
}

/// An update computed for a voice that is gone, its slot taken by another
/// 3D voice, leaves that voice alone (SoLoud's `update3dAudio` applies the
/// old voice's gains to whatever voice holds the slot): the gone voice,
/// stopped or ended and its final event unread, is moved far enough away to
/// be killed, and the new one sounds as it would alone.
#[test]
fn updates_reach_only_their_own_voice() {
    for stopped in [true, false] {
        let mut far = tone(if stopped { 20000 } else { 100 }, 1, 44100.0, 41);
        far.set_3d_attenuation(Attenuation::InverseDistance, 1.0);
        far.set_inaudible_behavior(false, true);
        let mut near = tone(20000, 1, 44100.0, 17);
        near.set_3d_attenuation(Attenuation::InverseDistance, 1.0);
        near.set_inaudible_behavior(false, true);
        let at = [1.5, 0.0, -2.0];
        let options = PlayOptions::default();

        let mut expected = Mixer::new(MixerConfig::default()).unwrap();
        expected.play_3d(&near, at, STILL, options);
        expected.update_3d_audio();
        let mut alone = vec![0.0; 2 * 512];
        expected.mix_interleaved(&mut alone);

        let (mut c, mut r) = realtime(RealtimeConfig::default());
        let id = c.play_3d(&far, [0.0, 0.0, -3.0], STILL, options).unwrap();
        render(&mut r, 256);
        if stopped {
            c.stop(id).unwrap();
        }
        let other = c.play_3d(&near, at, STILL, options).unwrap();
        // The gone voice's final event has not been read: the update still
        // carries it, now far enough away to be killed.
        c.set_3d_source_position(id, [0.0, 0.0, -100000.0]);
        c.update_3d_audio().unwrap();
        assert_eq!(render(&mut r, 512), alone, "stopped: {stopped}");
        let seen = events(&mut c);
        let reason = if stopped {
            FinishReason::Stopped
        } else {
            FinishReason::Ended
        };
        assert!(seen.contains(&Event::Finished(id, reason)));
        assert!(seen.contains(&Event::Started(other)));
        assert!(!seen.iter().any(|e| e.id() == other && e.is_final()));
    }
}

/// A 3D play too quiet to start, and a voice an update makes inaudible,
/// finish as stopped when their inaudible behavior kills.
#[test]
fn inaudible_kills_report_stopped() {
    let mut sound = tone(20000, 1, 44100.0, 41);
    sound.set_3d_attenuation(Attenuation::InverseDistance, 1.0);
    sound.set_inaudible_behavior(false, true);
    let (mut c, mut r) = realtime(RealtimeConfig::default());
    let options = PlayOptions::default();
    let quiet = c
        .play_3d(&sound, [0.0, 0.0, -500.0], STILL, options)
        .unwrap();
    let near = c.play_3d(&sound, [0.0, 0.0, -2.0], STILL, options).unwrap();
    render(&mut r, 64);
    assert_eq!(
        events(&mut c),
        [
            Event::Started(quiet),
            Event::Finished(quiet, FinishReason::Stopped),
            Event::Started(near),
        ]
    );
    c.set_3d_source_position(near, [0.0, 0.0, -5000.0]);
    c.update_3d_audio().unwrap();
    render(&mut r, 64);
    assert_eq!(
        events(&mut c),
        [Event::Finished(near, FinishReason::Stopped)]
    );
    // Its final event read, the voice is forgotten.
    assert!(c.spatial.voice_mut(near).is_none());
}

/// At most `spatial_batches` updates are in flight; the renderer hands each
/// back once applied.
#[test]
fn updates_in_flight_are_bounded() {
    let config = RealtimeConfig {
        spatial_batches: 2,
        ..RealtimeConfig::default()
    };
    let (mut c, mut r) = realtime(config);
    c.update_3d_audio().unwrap();
    c.update_3d_audio().unwrap();
    assert_eq!(c.update_3d_audio(), Err(ControllerError::QueueFull));
    render(&mut r, 16);
    c.update_3d_audio().unwrap();
    c.update_3d_audio().unwrap();
    assert_eq!(c.update_3d_audio(), Err(ControllerError::QueueFull));
    drop(r);
    assert_eq!(c.update_3d_audio(), Err(ControllerError::Disconnected));
}

/// Setters change only voices played in 3D through the controller, and
/// reject what the mixer rejects.
#[test]
fn setters_follow_the_mixer() {
    let (mut c, mut r) = realtime(RealtimeConfig::default());
    let plain = c.play(&tone(5000, 1, 44100.0, 9)).unwrap();
    c.set_3d_source_position(plain, [1.0, 0.0, 0.0]);
    assert!(c.spatial.voice_mut(plain).is_none());
    assert_eq!(
        c.set_3d_sound_speed(f32::INFINITY),
        Err(ControllerError::InvalidSoundSpeed)
    );
    assert_eq!(
        c.set_3d_sound_speed(-1.0),
        Err(ControllerError::InvalidSoundSpeed)
    );
    c.set_3d_sound_speed(100.0).unwrap();
    assert_eq!(c.sound_speed_3d(), 100.0);
    assert_eq!(c.speaker_position(1), Some([-2.0, 0.0, 1.0]));
    assert_eq!(c.speaker_position(2), None);
    assert_eq!(
        c.set_speaker_position(2, STILL),
        Err(ControllerError::NoSuchSpeaker)
    );
    c.set_speaker_position(1, [0.0, 0.0, 1.0]).unwrap();
    assert_eq!(c.speaker_position(1), Some([0.0, 0.0, 1.0]));
    render(&mut r, 16);
}

/// The listener and speakers set on the mixer before `split` carry over.
#[test]
fn split_keeps_the_mixer_scene() {
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.set_3d_listener_position([3.0, 0.0, 0.0]);
    mixer.set_speaker_position(0, [1.0, 0.0, 0.0]).unwrap();
    mixer.set_3d_sound_speed(50.0).unwrap();
    let (c, _r) = split(mixer, RealtimeConfig::default());
    assert_eq!(c.speaker_position(0), Some([1.0, 0.0, 0.0]));
    assert_eq!(c.sound_speed_3d(), 50.0);
    assert_eq!(c.spatial.scene.position.x, 3.0);
}

/// Counts its calls.
#[derive(Debug, Default)]
struct Count(std::sync::atomic::AtomicU32);

impl Collider for Count {
    fn collide(&self, _voice: &SpatialVoice, _user_data: i32) -> f32 {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        1.0
    }
}

/// Every voice's gains are computed before any applies: the child of a 3D
/// bus that the update kills still has its collider run, as in SoLoud.
#[test]
fn a_killed_bus_stops_its_voices_after_their_colliders_ran() {
    use std::sync::atomic::Ordering::Relaxed;
    let count = Arc::new(Count::default());
    let mut sound = tone(20000, 1, 44100.0, 41);
    sound.set_3d_collider(Some(count.clone()), 0);
    let mut bus = crate::Bus::new();
    bus.set_3d_min_max_distance(1.0, 20.0);
    bus.set_3d_attenuation(Attenuation::LinearDistance, 1.0);
    bus.set_inaudible_behavior(false, true);
    let (mut c, mut r) = realtime(RealtimeConfig::default());
    let options = PlayOptions::default();
    let keep = c.play_3d(&sound, [1.0, 0.0, -3.0], STILL, options).unwrap();
    let bus_voice = c.play_3d(&bus, [0.0, 0.0, -5.0], STILL, options).unwrap();
    render(&mut r, 64);
    let child = c
        .bus_play_3d(&bus, &sound, [-1.0, 0.0, -2.0], STILL, options)
        .unwrap();
    render(&mut r, 64);
    assert_eq!(count.0.load(Relaxed), 2);
    events(&mut c);
    c.set_3d_source_position(bus_voice, [0.0, 0.0, -30.0]);
    c.update_3d_audio().unwrap();
    assert_eq!(count.0.load(Relaxed), 4, "the child's collider ran");
    render(&mut r, 64);
    let finished: Vec<VoiceId> = events(&mut c)
        .iter()
        .filter(|e| e.is_final())
        .map(|e| e.id())
        .collect();
    assert!(finished.contains(&bus_voice) && finished.contains(&child));
    assert!(!finished.contains(&keep));
    c.update_3d_audio().unwrap();
    assert_eq!(count.0.load(Relaxed), 5);
}
