//! The playback sync's 3D part: sounds of entities with a
//! [`SpatialEmitter`](crate::SpatialEmitter) start in 3D, and each frame the
//! listener and the playing emitters are sent to the controller, followed
//! by one 3D update.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use solouder_core::{Controller, ControllerError, PlayOptions, Playable, Sound, Stream, VoiceId};

use super::records::Record;
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

/// A source the plugin plays: a decoded sound or a stream.
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

/// Sends every playing 3D sound's emitter, then asks for one 3D update.
/// A sound whose emitter is gone keeps its last place but is still updated
/// as the listener moves. A full queue retries next frame.
pub(super) fn send_emitters(
    controller: &mut Controller,
    snapshot: &SpatialSnapshot,
    records: &HashMap<Entity, Record>,
) {
    let mut any = false;
    for (entity, emitter) in &snapshot.emitters {
        let Some(voice) = records
            .get(entity)
            .filter(|record| record.spatial)
            .and_then(|record| record.voice)
        else {
            continue;
        };
        any = true;
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
    if any || records.values().any(|r| r.spatial && r.voice.is_some()) {
        let _ = controller.update_3d_audio();
    }
}
