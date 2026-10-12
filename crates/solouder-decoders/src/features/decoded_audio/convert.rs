//! Stored samples to the `f32` the mixer plays.
//!
//! Except for 8-bit audio, these are the conversions SoLoud's WAV loader
//! (dr_wav) uses, so decoded files match the reference bit for bit.

use super::riff::Encoding;

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

/// G.711 A-law to 16-bit linear (the values of dr_wav's `g_drwavAlawTable`).
pub(crate) fn alaw(code: u8) -> i16 {
    let code = code ^ 0x55;
    let mantissa = i16::from(code & 0x0F) << 4;
    let segment = (code & 0x70) >> 4;
    let magnitude = match segment {
        0 => mantissa + 8,
        _ => (mantissa + 0x108) << (segment - 1),
    };
    if code & 0x80 != 0 {
        magnitude
    } else {
        -magnitude
    }
}

/// G.711 mu-law to 16-bit linear (the values of dr_wav's
/// `g_drwavMulawTable`).
pub(crate) fn mulaw(code: u8) -> i16 {
    const BIAS: i16 = 0x84;
    let code = !code;
    let magnitude = ((i16::from(code & 0x0F) << 3) + BIAS) << ((code & 0x70) >> 4);
    if code & 0x80 != 0 {
        BIAS - magnitude
    } else {
        magnitude - BIAS
    }
}

/// Converts interleaved `data` into `out`, one plane per channel. `out` holds
/// whole frames; `data` must hold at least as many samples.
pub(crate) fn deinterleave(encoding: Encoding, data: &[u8], channels: usize, out: &mut [f32]) {
    match encoding {
        Encoding::U8 => planar(data, channels, out, |[s]| from_u8(s)),
        Encoding::S16 => planar(data, channels, out, |s| from_i16(i16::from_le_bytes(s))),
        Encoding::S24 => planar(data, channels, out, |[a, b, c]| {
            from_i24(i32::from_le_bytes([0, a, b, c]) >> 8)
        }),
        Encoding::S32 => planar(data, channels, out, |s| from_i32(i32::from_le_bytes(s))),
        Encoding::F32 => planar(data, channels, out, |s| from_f32(f32::from_le_bytes(s))),
        Encoding::F64 => planar(data, channels, out, |s| from_f64(f64::from_le_bytes(s))),
        Encoding::ALaw => planar(data, channels, out, |[s]| from_i16(alaw(s))),
        Encoding::MuLaw => planar(data, channels, out, |[s]| from_i16(mulaw(s))),
    }
}

fn planar<const W: usize>(
    data: &[u8],
    channels: usize,
    out: &mut [f32],
    convert: impl Fn([u8; W]) -> f32,
) {
    let frames = out.len() / channels;
    let (samples, _) = data.as_chunks::<W>();
    for (i, &sample) in samples.iter().take(frames * channels).enumerate() {
        out[(i % channels) * frames + i / channels] = convert(sample);
    }
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

    #[test]
    fn companding_matches_the_g711_tables() {
        // First, middle and last entries of dr_wav's tables.
        assert_eq!(alaw(0x00), -5504);
        assert_eq!(alaw(0x55), -8);
        assert_eq!(alaw(0xD5), 8);
        assert_eq!(alaw(0x80), 5504);
        assert_eq!(alaw(0xAA), 32256);
        assert_eq!(alaw(0x2A), -32256);
        assert_eq!(mulaw(0x00), -32124);
        assert_eq!(mulaw(0x7F), 0);
        assert_eq!(mulaw(0xFF), 0);
        assert_eq!(mulaw(0x80), 32124);
        assert_eq!(mulaw(0x7E), -8);
    }

    #[test]
    fn s24_is_sign_extended() {
        let mut out = [0.0; 2];
        deinterleave(
            Encoding::S24,
            &[0x00, 0x00, 0x80, 0xFF, 0xFF, 0x7F],
            1,
            &mut out,
        );
        assert_eq!(out, [-1.0, 8_388_607.0 / 8_388_608.0]);
    }

    #[test]
    fn channels_become_planes() {
        let data: Vec<u8> = [1i16, -1, 2, -2, 3, -3]
            .iter()
            .flat_map(|s| (s * 1024).to_le_bytes())
            .collect();
        let mut out = [0.0; 6];
        deinterleave(Encoding::S16, &data, 2, &mut out);
        assert_eq!(out, [1.0, 2.0, 3.0, -1.0, -2.0, -3.0].map(|v| v / 32.0));
    }
}
