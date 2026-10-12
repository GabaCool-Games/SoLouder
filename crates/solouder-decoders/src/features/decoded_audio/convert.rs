//! Integer and float samples to the `f32` the mixer plays.
//!
//! Except for 8-bit audio, these are the conversions SoLoud's WAV loader
//! (dr_wav) uses, so decoded files match the reference bit for bit.

use symphonia::core::audio::sample::Sample;
use symphonia::core::audio::{Audio, AudioBuffer, GenericAudioBufferRef};

use super::DecodeError;

/// Unsigned 8-bit, centered on 128. dr_wav scales by 2/255 instead, which maps
/// silence (128) to +1/255.
pub(crate) fn from_u8(sample: u8) -> f32 {
    (f32::from(sample) - 128.0) / 128.0
}

/// `drwav_s16_to_f32` (also used after A-law and mu-law expansion). dr_wav
/// multiplies by 2^-15, which is the same as this exact division.
pub(crate) fn from_i16(sample: i16) -> f32 {
    f32::from(sample) / 32_768.0
}

/// `drwav_s24_to_f32`: exact, since every 24-bit value fits an `f32`.
pub(crate) fn from_i24(sample: i32) -> f32 {
    (f64::from(sample) / 8_388_608.0) as f32
}

/// `drwav_s32_to_f32`: divide in `f64`, then round once.
pub(crate) fn from_i32(sample: i32) -> f32 {
    (f64::from(sample) / 2_147_483_648.0) as f32
}

/// Non-finite samples would poison every voice they are mixed with; they
/// decode as silence. dr_wav passes them through.
pub(crate) fn from_f32(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

/// `drwav_f64_to_f32`, with values outside the `f32` range treated as
/// non-finite.
pub(crate) fn from_f64(sample: f64) -> f32 {
    from_f32(sample as f32)
}

/// Appends every plane of `buffer` to the matching entry of `planes`.
pub(crate) fn append(
    buffer: &GenericAudioBufferRef<'_>,
    planes: &mut [Vec<f32>],
) -> Result<(), DecodeError> {
    match buffer {
        GenericAudioBufferRef::U8(buf) => append_with(buf, planes, |s| from_u8(*s)),
        GenericAudioBufferRef::S16(buf) => append_with(buf, planes, |s| from_i16(*s)),
        GenericAudioBufferRef::S24(buf) => append_with(buf, planes, |s| from_i24(s.inner())),
        GenericAudioBufferRef::S32(buf) => append_with(buf, planes, |s| from_i32(*s)),
        GenericAudioBufferRef::F32(buf) => append_with(buf, planes, |s| from_f32(*s)),
        GenericAudioBufferRef::F64(buf) => append_with(buf, planes, |s| from_f64(*s)),
        _ => Err(DecodeError::Unsupported(
            "sample format not used by WAV".to_owned(),
        )),
    }
}

fn append_with<S: Sample>(
    buffer: &AudioBuffer<S>,
    planes: &mut [Vec<f32>],
    convert: impl Fn(&S) -> f32,
) -> Result<(), DecodeError> {
    if buffer.num_planes() != planes.len() {
        return Err(DecodeError::Malformed(
            "decoder changed the channel count".to_owned(),
        ));
    }
    for (index, plane) in planes.iter_mut().enumerate() {
        let source = buffer
            .plane(index)
            .ok_or_else(|| DecodeError::Malformed("missing channel".to_owned()))?;
        plane
            .try_reserve(source.len())
            .map_err(|_| DecodeError::OutOfMemory)?;
        plane.extend(source.iter().map(&convert));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u8_is_centered_on_128() {
        assert_eq!(from_u8(128), 0.0);
        assert_eq!(from_u8(0), -1.0);
        assert_eq!(from_u8(255), 127.0 / 128.0);
    }

    #[test]
    fn integers_scale_to_full_range() {
        assert_eq!(from_i16(i16::MIN), -1.0);
        assert_eq!(from_i16(16384), 0.5);
        assert_eq!(from_i24(-8_388_608), -1.0);
        assert_eq!(from_i24(1), 1.0 / 8_388_608.0);
        assert_eq!(from_i32(i32::MIN), -1.0);
        // Rounded once from the exact quotient, as dr_wav does.
        assert_eq!(from_i32(i32::MAX), 1.0);
        assert_eq!(
            from_i32(0x0123_4567),
            (19_088_743.0f64 / 2_147_483_648.0) as f32
        );
    }

    #[test]
    fn non_finite_floats_become_silence() {
        assert_eq!(from_f32(f32::NAN), 0.0);
        assert_eq!(from_f32(f32::NEG_INFINITY), 0.0);
        assert_eq!(from_f32(1.5), 1.5);
        assert_eq!(from_f64(1e300), 0.0);
        assert_eq!(from_f64(0.1), 0.1f32);
    }
}
