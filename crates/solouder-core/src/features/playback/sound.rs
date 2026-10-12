// Ported from SoLoud src/core/soloud_audiosource.cpp and
// src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use crate::contracts::Pcm;

/// Something that can be played: prepared PCM plus its default playback
/// settings. Corresponds to a loaded SoLoud `Wav`.
///
/// Cloning is cheap; the samples are shared. Each `play` starts an independent
/// voice with its own position.
#[derive(Clone, Debug)]
pub struct Sound {
    pcm: Pcm,
    volume: f32,
    looping: bool,
}

impl Sound {
    /// A sound at volume 1 that does not loop.
    pub fn new(pcm: Pcm) -> Self {
        Self {
            pcm,
            volume: 1.0,
            looping: false,
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

    /// Volume used by `play` when no volume is given (`AudioSource::setVolume`).
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Sets the default volume for voices started after this call.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }

    /// Whether new voices loop back to the start (`AudioSource::setLooping`).
    pub fn looping(&self) -> bool {
        self.looping
    }

    /// Sets whether voices started after this call loop.
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }
}
