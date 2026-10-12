use super::*;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/wav/",
            $name
        ))
        .as_slice()
    };
}

fn decode(bytes: &[u8]) -> Result<solouder_core::Pcm, DecodeError> {
    decode_wav(bytes, &DecodeLimits::default())
}

/// A plain-header WAV file around `data`, whose data chunk claims `declared`
/// bytes.
fn wav(format: u16, channels: u16, rate: u32, bits: u16, data: &[u8], declared: u32) -> Vec<u8> {
    let block = channels * bits / 8;
    let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&declared.to_le_bytes());
    // A cut-off file still declares its original RIFF size.
    let riff = (out.len() - 8) as u64 + u64::from(declared);
    out.extend_from_slice(data);
    let riff = u32::try_from(riff).unwrap_or(u32::MAX);
    out[4..8].copy_from_slice(&riff.to_le_bytes());
    out
}

fn le_bytes<const N: usize, T: Copy>(values: &[T], to_bytes: fn(T) -> [u8; N]) -> Vec<u8> {
    values.iter().flat_map(|v| to_bytes(*v)).collect()
}

#[test]
fn decodes_every_supported_sample_format() {
    let cases: [(&[u8], u32, u32, f32); 11] = [
        (fixture!("u8_mono_8000.wav"), 1, 1000, 8000.0),
        (fixture!("s16_stereo_44100.wav"), 2, 1500, 44100.0),
        (fixture!("s24_mono_48000.wav"), 1, 700, 48000.0),
        (fixture!("s32_stereo_22050.wav"), 2, 600, 22050.0),
        (fixture!("f32_quad_48000.wav"), 4, 520, 48000.0),
        (fixture!("f64_mono_96000.wav"), 1, 300, 96000.0),
        (fixture!("alaw_mono_8000.wav"), 1, 512, 8000.0),
        (fixture!("mulaw_mono_8000.wav"), 1, 512, 8000.0),
        (fixture!("ext_s16_5.1_48000.wav"), 6, 400, 48000.0),
        (fixture!("ext_f32_7.1_44100.wav"), 8, 300, 44100.0),
        (fixture!("ext_s24in32_stereo_32000.wav"), 2, 256, 32000.0),
    ];
    for (bytes, channels, frames, rate) in cases {
        let pcm = decode(bytes).unwrap();
        assert_eq!(
            (pcm.channels(), pcm.frames(), pcm.sample_rate()),
            (channels, frames, rate)
        );
        for channel in 0..channels {
            assert!(pcm.channel(channel).iter().all(|s| s.is_finite()));
        }
    }
}

#[test]
fn integer_extremes_map_to_full_scale() {
    // The generator starts every integer fixture with min, max, 0, -1,
    // rotated by channel.
    let s16 = decode(fixture!("s16_stereo_44100.wav")).unwrap();
    assert_eq!(
        &s16.channel(0)[..4],
        &[-1.0, 32767.0 / 32768.0, 0.0, -1.0 / 32768.0]
    );
    assert_eq!(s16.channel(1)[0], 32767.0 / 32768.0);
    let s24 = decode(fixture!("s24_mono_48000.wav")).unwrap();
    assert_eq!(s24.channel(0)[0], -1.0);
    let wide = decode(fixture!("ext_s24in32_stereo_32000.wav")).unwrap();
    assert_eq!(&wide.channel(0)[..2], &[-1.0, 8_388_607.0 / 8_388_608.0]);
    let u8 = decode(fixture!("u8_mono_8000.wav")).unwrap();
    assert_eq!(u8.channel(0)[0], -1.0);
}

#[test]
fn extra_chunks_and_padding_are_skipped() {
    let pcm = decode(fixture!("chunks_s16_stereo_11025.wav")).unwrap();
    assert_eq!((pcm.channels(), pcm.frames()), (2, 333));
    assert_eq!(pcm.channel(0)[0], -1.0);
}

#[test]
fn rejects_unplayable_layouts() {
    assert!(matches!(
        decode(fixture!("bad_nine_channels.wav")),
        Err(DecodeError::UnsupportedChannelCount(9))
    ));
    assert!(matches!(
        decode(fixture!("bad_no_frames.wav")),
        Err(DecodeError::Empty)
    ));
    assert!(matches!(
        decode(fixture!("bad_zero_rate.wav")),
        Err(DecodeError::InvalidSampleRate)
    ));
    assert!(matches!(
        decode(fixture!("bad_cut_header.wav")),
        Err(DecodeError::Malformed(_))
    ));
    assert!(matches!(
        decode(b"OggS not a wave file"),
        Err(DecodeError::NotRecognized)
    ));
    assert!(matches!(decode(b""), Err(DecodeError::NotRecognized)));
}

#[test]
fn short_data_chunk_keeps_the_frames_present() {
    let values: Vec<i16> = (0..600).map(|s| s * 50).collect();
    let data = le_bytes(&values, i16::to_le_bytes);
    let pcm = decode(&wav(1, 1, 8000, 16, &data, 2000)).unwrap();
    assert_eq!(pcm.frames(), 600);
    assert_eq!(pcm.channel(0)[599], f32::from(599i16 * 50) / 32768.0);
    // SoLoud keeps the declared 1000 frames, the last 400 silent.
    let cut = decode(fixture!("short_data_s16_mono_22050.wav")).unwrap();
    assert_eq!(cut.frames(), 600);
}

#[test]
fn partial_trailing_frame_is_dropped() {
    let mut data = le_bytes(&[100i16, -100, 200, -200], i16::to_le_bytes);
    data.push(7);
    let pcm = decode(&wav(1, 2, 8000, 16, &data, data.len() as u32)).unwrap();
    assert_eq!(pcm.frames(), 2);
}

#[test]
fn non_finite_floats_decode_as_silence() {
    let data = le_bytes(&[0.5f32, f32::NAN, f32::INFINITY, -0.25], f32::to_le_bytes);
    let pcm = decode(&wav(3, 1, 8000, 32, &data, 16)).unwrap();
    assert_eq!(pcm.channel(0), &[0.5, 0.0, 0.0, -0.25]);
}

#[test]
fn limit_stops_large_files_while_decoding() {
    let limits = DecodeLimits { max_samples: 1000 };
    let result = decode_wav(fixture!("s16_stereo_44100.wav"), &limits);
    assert!(matches!(
        result,
        Err(DecodeError::TooLarge { limit: 1000, .. })
    ));
    let exact = DecodeLimits { max_samples: 3000 };
    assert!(decode_wav(fixture!("s16_stereo_44100.wav"), &exact).is_ok());
}

#[test]
fn header_claims_do_not_drive_allocation() {
    let pcm = decode(&wav(1, 2, 8000, 16, &[0u8; 8], u32::MAX - 1)).unwrap();
    assert_eq!(pcm.frames(), 2);
}
