// Ported from SoLoud src/core/soloud_audiosource.cpp and
// src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::sync::atomic::{AtomicU64, Ordering};

use super::source::private::{Prepared, Sealed};
use super::source::{Playable, SourceSettings, source_settings_api};
use super::voice::{Voice, VoiceSource};
use crate::contracts::Pcm;

/// Identifies a [`Sound`] or [`Bus`](super::Bus) and its clones, so their
/// voices can be stopped or counted together (`mAudioSourceID`). Unique
/// within the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SoundId(u64);

impl SoundId {
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// The number behind the ID; never 0.
    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

/// Something that can be played: prepared PCM plus its default playback
/// settings. Corresponds to a loaded SoLoud `Wav`.
///
/// Cloning is cheap; the samples are shared. A clone keeps the original's
/// [`SoundId`], as a copied SoLoud `AudioSource` keeps its ID, so
/// [`Mixer::stop_sound`] stops the voices of both. Each `play` starts an
/// independent voice with its own position.
#[derive(Clone, Debug)]
pub struct Sound {
    pcm: Pcm,
    id: SoundId,
    settings: SourceSettings,
}

impl Sound {
    /// A sound at volume 1 that does not loop, with a new [`SoundId`].
    pub fn new(pcm: Pcm) -> Self {
        Self {
            pcm,
            id: SoundId::next(),
            settings: SourceSettings::default(),
        }
    }

    /// The samples, without the playback settings.
    pub fn into_pcm(self) -> Pcm {
        self.pcm
    }

    /// The samples this sound plays.
    pub fn pcm(&self) -> &Pcm {
        &self.pcm
    }

    /// Shared by this sound and its clones.
    pub fn id(&self) -> SoundId {
        self.id
    }

    /// Seconds of audio at the sound's sample rate (`Wav::getLength`).
    pub fn length(&self) -> f64 {
        // SoLoud divides in single precision.
        f64::from(self.pcm.frames() as f32 / self.pcm.sample_rate())
    }

    source_settings_api!();

    /// A voice at the start of the sound, before `play` applies its options.
    pub(crate) fn voice(&self) -> Voice {
        let source = VoiceSource::Pcm {
            pcm: self.pcm.clone(),
            offset: 0,
        };
        Voice::new(&self.settings, self.id, source, self.pcm.sample_rate(), 0)
    }
}

impl Playable for Sound {
    fn source_id(&self) -> SoundId {
        self.id
    }
}

impl Sealed for Sound {
    fn filter_slots(&self) -> Option<std::sync::Arc<super::source::FilterSlots>> {
        self.settings.filters.clone()
    }

    fn prepare(&self) -> Prepared {
        Prepared {
            voice: self.voice(),
            stop_others: self.settings.single_instance,
            default_volume: self.settings.volume,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_an_id_and_new_sounds_do_not() {
        let pcm = Pcm::from_planar(vec![0.0; 4], 1, 8000.0).unwrap();
        let a = Sound::new(pcm.clone());
        let mut b = a.clone();
        b.set_looping(true);
        assert_eq!(a.id(), b.id());
        assert_ne!(a.id(), Sound::new(pcm).id());
    }

    #[test]
    fn length_divides_in_single_precision() {
        let pcm = Pcm::from_planar(vec![0.0; 44101], 1, 44100.0).unwrap();
        assert_eq!(Sound::new(pcm).length(), f64::from(44101.0_f32 / 44100.0));
    }
}
