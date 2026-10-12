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
    let block = channels.wrapping_mul(bits / 8);
    let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&format.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&rate.wrapping_mul(u32::from(block)).to_le_bytes());
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

/// An extensible-header WAV file.
fn wav_ext(sub: u16, channels: u16, bits: u16, valid: u16, mask: u32, data: &[u8]) -> Vec<u8> {
    let block = channels.wrapping_mul(bits / 8);
    let mut fmt = Vec::new();
    for v in [0xFFFE_u16, channels] {
        fmt.extend_from_slice(&v.to_le_bytes());
    }
    fmt.extend_from_slice(&8000u32.to_le_bytes());
    fmt.extend_from_slice(&(8000 * u32::from(block)).to_le_bytes());
    for v in [block, bits, 22, valid] {
        fmt.extend_from_slice(&v.to_le_bytes());
    }
    fmt.extend_from_slice(&mask.to_le_bytes());
    fmt.extend_from_slice(&sub.to_le_bytes());
    fmt.extend_from_slice(&[0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]);
    riff(&[(b"fmt ", &fmt), (b"data", data)])
}

fn riff(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut body = b"WAVE".to_vec();
    for (id, data) in chunks {
        body.extend_from_slice(*id);
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        if data.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

#[test]
fn absurd_channel_counts_are_rejected_without_overflow() {
    // 65535 channels of 32-bit samples overflow a 16-bit block size.
    let plain = wav(1, u16::MAX, 8000, 32, &[0; 64], 64);
    assert!(matches!(
        decode(&plain),
        Err(DecodeError::UnsupportedChannelCount(65535))
    ));
    // A zero mask must not turn 33 or 40 channels into 1 or 8.
    for channels in [33, 40] {
        let ext = wav_ext(1, channels, 16, 16, 0, &[0; 160]);
        assert!(matches!(
            decode(&ext),
            Err(DecodeError::UnsupportedChannelCount(c)) if c == u32::from(channels)
        ));
    }
    assert!(matches!(
        decode(&wav(1, 0, 8000, 16, &[0; 8], 8)),
        Err(DecodeError::UnsupportedChannelCount(0))
    ));
}

#[test]
fn the_channel_mask_is_ignored() {
    let data = le_bytes(&[1000i16, -1000, 2000, -2000], i16::to_le_bytes);
    for mask in [0, 0x8000_0000, u32::MAX] {
        let pcm = decode(&wav_ext(1, 2, 16, 16, mask, &data)).unwrap();
        assert_eq!((pcm.channels(), pcm.frames()), (2, 2));
        assert_eq!(pcm.channel(1)[1], -2000.0 / 32768.0);
    }
}

#[test]
fn extensible_companded_audio_decodes() {
    let codes: Vec<u8> = (0..=255).collect();
    let plain_alaw = decode(&wav(6, 1, 8000, 8, &codes, 256)).unwrap();
    let ext_alaw = decode(&wav_ext(6, 1, 8, 8, 0x4, &codes)).unwrap();
    assert_eq!(plain_alaw.channel(0), ext_alaw.channel(0));
    let plain_mulaw = decode(&wav(7, 1, 8000, 8, &codes, 256)).unwrap();
    let ext_mulaw = decode(&wav_ext(7, 1, 8, 8, 0x4, &codes)).unwrap();
    assert_eq!(plain_mulaw.channel(0), ext_mulaw.channel(0));
    assert_ne!(plain_alaw.channel(0), plain_mulaw.channel(0));
}

#[test]
fn unsupported_widths_and_formats_are_reported_as_unsupported() {
    for (format, bits) in [(1, 12), (1, 64), (3, 16), (6, 16), (2, 4)] {
        let result = decode(&wav(format, 1, 8000, bits, &[0; 16], 16));
        assert!(
            matches!(result, Err(DecodeError::Unsupported(_))),
            "format {format} bits {bits}: {result:?}"
        );
    }
}

#[test]
fn broken_layouts_are_malformed_not_panics() {
    let short_fmt = riff(&[(b"fmt ", &[1, 0, 1, 0]), (b"data", &[0; 4])]);
    assert!(matches!(decode(&short_fmt), Err(DecodeError::Malformed(_))));
    let mut short_ext = wav_ext(1, 1, 16, 16, 4, &[0; 4]);
    short_ext[16] = 20; // the fmt chunk now claims 20 bytes
    assert!(decode(&short_ext).is_err());
    let data_first = riff(&[(b"data", &[0; 4]), (b"fmt ", &[0; 16])]);
    assert!(matches!(
        decode(&data_first),
        Err(DecodeError::Malformed(_))
    ));
    // A chunk claiming 4 GiB is skipped past the end of the file.
    let mut huge = b"RIFF\0\0\0\0WAVELIST".to_vec();
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    huge.extend_from_slice(&[0; 32]);
    assert!(matches!(decode(&huge), Err(DecodeError::Malformed(_))));
}

#[test]
fn mutated_files_never_panic() {
    // A small deterministic fuzzer: flip, overwrite and truncate bytes of
    // every fixture. Any panic fails the test.
    let seeds: [&[u8]; 6] = [
        fixture!("s16_stereo_44100.wav"),
        fixture!("ext_f32_7.1_44100.wav"),
        fixture!("ext_s24in32_stereo_32000.wav"),
        fixture!("chunks_s16_stereo_11025.wav"),
        fixture!("alaw_mono_8000.wav"),
        fixture!("f64_mono_96000.wav"),
    ];
    let mut state = 0x2545_F491_u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as usize
    };
    let limits = DecodeLimits {
        max_samples: 1 << 20,
    };
    for round in 0..20_000 {
        let mut bytes = seeds[round % seeds.len()].to_vec();
        for _ in 0..1 + next() % 4 {
            // Mostly the first 80 bytes, where the headers are.
            let at = if next() % 4 == 0 {
                next() % bytes.len()
            } else {
                next() % 80
            };
            bytes[at] = match next() % 3 {
                0 => bytes[at] ^ (1 << (next() % 8)),
                1 => 0xFF,
                _ => next() as u8,
            };
        }
        if next() % 5 == 0 {
            bytes.truncate(next() % bytes.len());
        }
        let _ = decode_wav(&bytes, &limits);
    }
}
