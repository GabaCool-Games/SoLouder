//! Visualization: the mix's wave, spectrum and channel volumes as a
//! resource, for meters and spectrum displays.
//!
//! Once per frame, in [`SolouderSystems::Sync`](crate::SolouderSystems), the
//! plugin sends [`AudioVisualization::enabled`] to the mixer when it
//! changed and, while recording, copies what the audio thread published at
//! its last render into the resource (the resource changes only when the
//! data does). The spectrum is computed on the main
//! thread. Turning recording off keeps the last data.

use bevy::prelude::*;
use solouder_core::{MAX_CHANNELS, WAVE_LEN};

use crate::SolouderSystems;
use crate::features::output_lifecycle::{OutputMode, SolouderController};

/// The mix's visualization data (SoLoud's `getWave`, `calcFFT` and
/// `getApproximateVolume`), refreshed every frame while
/// [`enabled`](Self::enabled).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct AudioVisualization {
    /// Whether the mixer records the data. Starts as the headless
    /// [`MixerConfig::visualization`](solouder_core::MixerConfig::visualization),
    /// or off with a device; set it to switch recording.
    pub enabled: bool,
    /// The first 256 frames of the latest recorded mix, summed over
    /// channels. Written by the plugin.
    pub wave: [f32; WAVE_LEN],
    /// Magnitudes of the first 256 bins of a 512-point FFT of
    /// [`wave`](Self::wave), zero-padded. Written by the plugin.
    pub spectrum: [f32; WAVE_LEN],
    /// Each output channel's peak over those frames; 0 past the output's
    /// channels. Written by the plugin.
    pub volumes: [f32; MAX_CHANNELS as usize],
}

impl AudioVisualization {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            wave: [0.0; WAVE_LEN],
            spectrum: [0.0; WAVE_LEN],
            volumes: [0.0; MAX_CHANNELS as usize],
        }
    }
}

/// The recording state the mixer has: from its configuration, then as
/// last sent.
#[derive(Resource)]
struct Recording(bool);

pub(crate) fn build(app: &mut App, output: &OutputMode) {
    let enabled = matches!(output, OutputMode::Headless(config) if config.visualization);
    app.insert_resource(AudioVisualization::new(enabled))
        .insert_resource(Recording(enabled))
        .add_systems(PostUpdate, sync_visualization.in_set(SolouderSystems::Sync));
}

fn sync_visualization(
    controller: Option<ResMut<SolouderController>>,
    mut recording: ResMut<Recording>,
    mut visual: ResMut<AudioVisualization>,
) {
    let Some(mut controller) = controller else {
        return;
    };
    let controller = controller.get();
    if !controller.is_connected() {
        return;
    }
    if recording.0 != visual.enabled && controller.set_visualization(visual.enabled).is_ok() {
        recording.0 = visual.enabled;
    }
    if !recording.0 {
        return;
    }
    let data = controller.visualization_data();
    if data.wave != visual.wave || data.volumes != visual.volumes {
        let visual = visual.as_mut();
        visual.spectrum = data.spectrum();
        visual.wave = data.wave;
        visual.volumes = data.volumes;
    }
}
