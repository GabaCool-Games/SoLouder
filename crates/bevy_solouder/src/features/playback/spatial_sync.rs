//! The playback sync's 3D part: sounds of entities with a
//! [`SpatialEmitter`](crate::SpatialEmitter) start in 3D, and each frame the
//! listener and the playing emitters are sent to the controller, followed
//! by one 3D update.

use bevy::prelude::*;
use solouder_core::{
    Controller, ControllerError, PlayOptions, Playable, Sound, Stream, Synth, VoiceId,
};

use super::records::Playback;
use crate::features::spatial::{EmitterPose, SpatialSnapshot};

/// Sends the listener and the speed of sound, before any sound starts so
/// new 3D sounds start with this frame's listener.
pub(super) fn send_listener(controller: &mut Controller, snapshot: &SpatialSnapshot) {
    let listener = &snapshot.listener;
    controller.set_3d_listener_parameters(
        listener.position.into(),
        listener.at.into(),
        listener.up.into(),
        listener.velocity.into(),
    );
    if controller.sound_speed_3d() != snapshot.sound_speed {
        // Unusable speeds are ignored, as documented.
        let _ = controller.set_3d_sound_speed(snapshot.sound_speed);
    }
}

/// A source the plugin plays: a decoded sound, a stream or a generator.
pub(super) trait Source: Playable {
    fn configure(&mut self, emitter: &EmitterPose);
}

macro_rules! source {
    ($type:ty) => {
        impl Source for $type {
            fn configure(&mut self, emitter: &EmitterPose) {
                let s = &emitter.settings;
                self.set_3d_attenuation(s.attenuation, s.rolloff);
                self.set_3d_min_max_distance(s.min_distance, s.max_distance);
                self.set_3d_doppler_factor(s.doppler_factor);
                self.set_3d_distance_delay(s.distance_delay);
            }
        }
    };
}
source!(Sound);
source!(Stream);
source!(Synth);

/// Plays `source`, in 3D at the emitter if there is one.
pub(super) fn play(
    controller: &mut Controller,
    source: &mut impl Source,
    options: PlayOptions,
    emitter: Option<&EmitterPose>,
) -> Result<VoiceId, ControllerError> {
    match emitter {
        Some(emitter) => {
            source.configure(emitter);
            let (position, velocity) = (emitter.position.into(), emitter.velocity.into());
            controller.play_3d(source, position, velocity, options)
        }
        None => controller.play_with(source, options),
    }
}

/// Sends the emitters of playing 3D sounds that moved or changed, then
/// asks for one 3D update if anything the update reads changed since the
/// last one the controller accepted: the listener, the speed of sound or
/// an emitter. A sound whose emitter is gone keeps its last place but is
/// still updated as the listener moves. A full queue retries next frame.
pub(super) fn send_emitters(
    controller: &mut Controller,
    snapshot: &SpatialSnapshot,
    playback: &mut Playback,
) {
    let mut changed = playback.update_pending;
    for (entity, emitter) in &snapshot.emitters {
        let Some(record) = playback.records.get_mut(entity) else {
            continue;
        };
        let Some(voice) = record.voice.filter(|_| record.spatial) else {
            continue;
        };
        if record.placed == Some(*emitter) {
            continue;
        }
        record.placed = Some(*emitter);
        changed = true;
        let s = &emitter.settings;
        controller.set_3d_source_parameters(
            voice,
            emitter.position.into(),
            emitter.velocity.into(),
        );
        controller.set_3d_source_min_max_distance(voice, s.min_distance, s.max_distance);
        controller.set_3d_source_attenuation(voice, s.attenuation, s.rolloff);
        controller.set_3d_source_doppler_factor(voice, s.doppler_factor);
    }
    let scene = (snapshot.listener, snapshot.sound_speed);
    changed |= playback.scene != Some(scene);
    let live = playback
        .records
        .values()
        .any(|record| record.spatial && record.voice.is_some());
    if !(changed && live) {
        return;
    }
    let accepted = controller.update_3d_audio().is_ok();
    playback.update_pending = !accepted;
    if accepted {
        playback.scene = Some(scene);
    }
}
