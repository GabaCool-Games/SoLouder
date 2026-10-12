// Ported from SoLoud src/core/soloud_audiosource.cpp and
// src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::contracts::Pcm;

/// Identifies a [`Sound`] and its clones, so their voices can be stopped or
/// counted together (`mAudioSourceID`). Unique within the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SoundId(u64);

impl SoundId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Something that can be played: prepared PCM plus its default playback
/// settings. Corresponds to a loaded SoLoud `Wav`.
///
/// Cloning is cheap; the samples are shared. A clone keeps the original's
/// [`SoundId`], as a copied SoLoud `AudioSource` keeps its ID, so
/// [`Mixer::stop_sound`](super::Mixer::stop_sound) stops the voices of both.
/// Each `play` starts an independent voice with its own position.
#[derive(Clone, Debug)]
pub struct Sound {
    pcm: Pcm,
    id: SoundId,
    volume: f32,
    looping: bool,
    loop_point: f64,
    auto_stop: bool,
    single_instance: bool,
    inaudible_tick: bool,
    inaudible_kill: bool,
}

impl Sound {
    /// A sound at volume 1 that does not loop, with a new [`SoundId`].
    pub fn new(pcm: Pcm) -> Self {
        Self {
            pcm,
            id: SoundId::next(),
            volume: 1.0,
            looping: false,
            loop_point: 0.0,
            auto_stop: true,
            single_instance: false,
            inaudible_tick: false,
            inaudible_kill: false,
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

    /// Volume used by `play` when no volume is given (`AudioSource::setVolume`).
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Sets the default volume for voices started after this call.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }

    /// Whether new voices loop (`AudioSource::setLooping`).
    pub fn looping(&self) -> bool {
        self.looping
    }

    /// Sets whether voices started after this call loop.
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// Seconds into the sound where looping voices restart; 0 by default.
    pub fn loop_point(&self) -> f64 {
        self.loop_point
    }

    /// Sets the loop point for voices started after this call
    /// (`AudioSource::setLoopPoint`).
    pub fn set_loop_point(&mut self, seconds: f64) {
        self.loop_point = seconds;
    }

    /// Whether voices stop once the sound has played out; true by default.
    pub fn auto_stop(&self) -> bool {
        self.auto_stop
    }

    /// Sets whether voices started after this call stop at the end of the
    /// sound or stay, silent, until stopped (`AudioSource::setAutoStop`).
    pub fn set_auto_stop(&mut self, auto_stop: bool) {
        self.auto_stop = auto_stop;
    }

    /// Whether playing the sound first stops its other voices on that mixer.
    pub fn single_instance(&self) -> bool {
        self.single_instance
    }

    /// Sets whether playing the sound first stops its other voices
    /// (`AudioSource::setSingleInstance`).
    pub fn set_single_instance(&mut self, single_instance: bool) {
        self.single_instance = single_instance;
    }

    /// Whether voices keep playing when inaudible (`tick`) and whether
    /// they are stopped then (`kill`).
    pub fn inaudible_behavior(&self) -> (bool, bool) {
        (self.inaudible_tick, self.inaudible_kill)
    }

    /// Sets the inaudible behavior of voices started after this call
    /// (`AudioSource::setInaudibleBehavior`). Ticking voices are always among
    /// the active voices.
    pub fn set_inaudible_behavior(&mut self, tick: bool, kill: bool) {
        self.inaudible_tick = tick;
        self.inaudible_kill = kill;
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
