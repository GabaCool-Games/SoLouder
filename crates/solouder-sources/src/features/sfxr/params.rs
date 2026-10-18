// Ported from SoLoud src/audiosource/sfxr/soloud_sfxr.cpp (zlib/libpng,
// Copyright (c) 2014 Jari Komppa), based on sfxr by Tomas Pettersson,
// relicensed under zlib/libpng by permission; altered for SoLouder.

//! Sfxr's settings (`SfxrParams`, `Sfxr::resetParams`) and the parameter
//! file format (`Sfxr::loadParamsFile`).

use std::fmt;

/// An sfxr sound's settings (SoLoud's `SfxrParams`, without the `p_`
/// prefixes). Most are 0 to 1; ramps and the arpeggio and phaser settings
/// are -1 to 1. Values outside those ranges are kept and computed with, as
/// in SoLoud.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SfxrParams {
    /// 0 square, 1 sawtooth, 2 sine, 3 noise; any other value renders 0
    /// before the filters.
    pub wave_type: i32,
    /// Start frequency.
    pub base_freq: f32,
    /// Frequency cutoff: the sound ends (or restarts, looping) when it slides below.
    pub freq_limit: f32,
    /// Frequency slide.
    pub freq_ramp: f32,
    /// Change of the frequency slide.
    pub freq_dramp: f32,
    /// Square duty cycle.
    pub duty: f32,
    /// Duty cycle sweep.
    pub duty_ramp: f32,
    /// Vibrato depth.
    pub vib_strength: f32,
    /// Vibrato speed.
    pub vib_speed: f32,
    /// Read from files and kept; nothing uses it, as in sfxr.
    pub vib_delay: f32,
    /// Envelope attack time.
    pub env_attack: f32,
    /// Envelope sustain time.
    pub env_sustain: f32,
    /// Envelope decay time.
    pub env_decay: f32,
    /// Extra volume at the start of the sustain.
    pub env_punch: f32,
    /// Read from files and kept; nothing uses it, as in sfxr.
    pub filter_on: bool,
    /// Low-pass filter resonance.
    pub lpf_resonance: f32,
    /// Low-pass filter cutoff; 1 turns the filter off.
    pub lpf_freq: f32,
    /// Low-pass cutoff sweep.
    pub lpf_ramp: f32,
    /// High-pass filter cutoff.
    pub hpf_freq: f32,
    /// High-pass cutoff sweep.
    pub hpf_ramp: f32,
    /// Phaser offset.
    pub pha_offset: f32,
    /// Phaser sweep.
    pub pha_ramp: f32,
    /// Restart rate; 0 never restarts.
    pub repeat_speed: f32,
    /// Arpeggio delay; 1 turns it off.
    pub arp_speed: f32,
    /// Arpeggio frequency change.
    pub arp_mod: f32,
    /// Master volume (0.05 unless changed in code; files keep it).
    pub master_vol: f32,
    /// Sound volume.
    pub sound_vol: f32,
}

impl Default for SfxrParams {
    /// `Sfxr::resetParams`: a short square tone.
    fn default() -> Self {
        Self {
            wave_type: 0,
            base_freq: 0.3,
            freq_limit: 0.0,
            freq_ramp: 0.0,
            freq_dramp: 0.0,
            duty: 0.0,
            duty_ramp: 0.0,
            vib_strength: 0.0,
            vib_speed: 0.0,
            vib_delay: 0.0,
            env_attack: 0.0,
            env_sustain: 0.3,
            env_decay: 0.4,
            env_punch: 0.0,
            filter_on: false,
            lpf_resonance: 0.0,
            lpf_freq: 1.0,
            lpf_ramp: 0.0,
            hpf_freq: 0.0,
            hpf_ramp: 0.0,
            pha_offset: 0.0,
            pha_ramp: 0.0,
            repeat_speed: 0.0,
            arp_speed: 0.0,
            arp_mod: 0.0,
            master_vol: 0.05,
            sound_vol: 0.5,
        }
    }
}

/// Why an sfxr parameter file did not load.
#[derive(Debug)]
#[non_exhaustive]
pub enum SfxrError {
    /// The file's version is not 100, 101 or 102.
    UnknownVersion(i32),
    /// The file ends before its last field (reference patch 0072: SoLoud
    /// loaded what was there).
    Truncated,
    /// The preset number is not 0 to 6.
    UnknownPreset(u32),
    /// Reading the file failed.
    Io(std::io::Error),
}

impl fmt::Display for SfxrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownVersion(v) => write!(f, "unknown sfxr file version {v}"),
            Self::Truncated => write!(f, "the sfxr file ends before its last field"),
            Self::UnknownPreset(p) => write!(f, "unknown sfxr preset {p} (expected 0 to 6)"),
            Self::Io(error) => write!(f, "reading the sfxr file failed: {error}"),
        }
    }
}

impl std::error::Error for SfxrError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// The longest file a loader reads: a version 102 file is 105 bytes.
pub(crate) const MAX_FILE: u64 = 128;

/// Little-endian fields, in order.
struct Fields<'a> {
    bytes: &'a [u8],
}

impl Fields<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], SfxrError> {
        let (head, rest) = self.bytes.split_first_chunk().ok_or(SfxrError::Truncated)?;
        self.bytes = rest;
        Ok(*head)
    }

    fn float(&mut self) -> Result<f32, SfxrError> {
        Ok(f32::from_le_bytes(self.take()?))
    }

    fn read(&mut self, field: &mut f32) -> Result<(), SfxrError> {
        *field = self.float()?;
        Ok(())
    }
}

impl SfxrParams {
    /// `self` with the settings an sfxr parameter file holds, as
    /// `Sfxr::loadParamsFile` reads them: a version (100, 101 or 102), the
    /// wave type, then `f32` fields, with the filter flag as one byte (any
    /// value but 0 is true; reference patch 0073). Version 102 adds the
    /// sound volume (otherwise 0.5); versions before 101 have no
    /// `freq_dramp`, `arp_speed` or `arp_mod`, which keep their values in
    /// `self`, as in SoLoud. Bytes after the last field are ignored.
    pub fn with_file(&self, bytes: &[u8]) -> Result<Self, SfxrError> {
        let mut fields = Fields { bytes };
        let version = i32::from_le_bytes(fields.take().map_err(|_| truncated_version(bytes))?);
        if !(100..=102).contains(&version) {
            return Err(SfxrError::UnknownVersion(version));
        }
        let mut p = *self;
        p.wave_type = i32::from_le_bytes(fields.take()?);
        p.sound_vol = 0.5;
        if version == 102 {
            fields.read(&mut p.sound_vol)?;
        }
        for field in [&mut p.base_freq, &mut p.freq_limit, &mut p.freq_ramp] {
            fields.read(field)?;
        }
        if version >= 101 {
            fields.read(&mut p.freq_dramp)?;
        }
        for field in [
            &mut p.duty,
            &mut p.duty_ramp,
            &mut p.vib_strength,
            &mut p.vib_speed,
            &mut p.vib_delay,
            &mut p.env_attack,
            &mut p.env_sustain,
            &mut p.env_decay,
            &mut p.env_punch,
        ] {
            fields.read(field)?;
        }
        p.filter_on = fields.take::<1>()?[0] != 0;
        for field in [
            &mut p.lpf_resonance,
            &mut p.lpf_freq,
            &mut p.lpf_ramp,
            &mut p.hpf_freq,
            &mut p.hpf_ramp,
            &mut p.pha_offset,
            &mut p.pha_ramp,
            &mut p.repeat_speed,
        ] {
            fields.read(field)?;
        }
        if version >= 101 {
            fields.read(&mut p.arp_speed)?;
            fields.read(&mut p.arp_mod)?;
        }
        Ok(p)
    }
}

/// SoLoud reads a short version into a zeroed `int`, so 1 to 3 bytes can
/// still spell 100, 101 or 102; the fields after it are then missing.
fn truncated_version(bytes: &[u8]) -> SfxrError {
    let mut word = [0; 4];
    word[..bytes.len()].copy_from_slice(bytes);
    match i32::from_le_bytes(word) {
        100..=102 => SfxrError::Truncated,
        version => SfxrError::UnknownVersion(version),
    }
}
