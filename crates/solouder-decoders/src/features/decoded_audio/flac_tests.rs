use super::*;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/flac/",
            $name
        ))
        .as_slice()
    };
}

fn decode_default(bytes: &[u8]) -> Result<solouder_core::Pcm, DecodeError> {
    decode_flac(bytes, &DecodeLimits::default())
}

/// The 4096-frame tone the damaged fixtures are made from, decoded from the
/// file that only overstates its length.
fn tone() -> Vec<f32> {
    let pcm = decode_default(fixture!("long_claim_s16_mono_44100.flac")).unwrap();
    pcm.channel(0).to_vec()
}

#[test]
fn decodes_every_layout() {
    let cases: [(&[u8], u32, u32, f32); 10] = [
        (fixture!("s16_stereo_44100.flac"), 2, 5108, 44100.0),
        (fixture!("s24_mono_48000.flac"), 1, 9192, 48000.0),
        (fixture!("s8_quad_8000.flac"), 4, 1500, 8000.0),
        (fixture!("s12_5.1_22050.flac"), 6, 3081, 22050.0),
        (fixture!("s20_stereo_96000_variable.flac"), 2, 6000, 96000.0),
        (fixture!("s16_7.1_32000_metadata.flac"), 8, 1200, 32000.0),
        (fixture!("s32_mono_44100.flac"), 1, 1500, 44100.0),
        (fixture!("s28_stereo_44100.flac"), 2, 1500, 44100.0),
        (fixture!("s32_stereo_44100.flac"), 2, 4572, 44100.0),
        (
            fixture!("s16_mono_44100_variable_equal.flac"),
            1,
            3072,
            44100.0,
        ),
    ];
    for (bytes, channels, frames, rate) in cases {
        let pcm = decode_default(bytes).unwrap();
        assert_eq!(
            (pcm.channels(), pcm.frames(), pcm.sample_rate()),
            (channels, frames, rate)
        );
        for channel in 0..channels {
            let plane = pcm.channel(channel);
            assert!(plane.iter().any(|&s| s != 0.0));
            assert!(plane.iter().all(|s| (-1.0..1.0).contains(s)));
        }
    }
}

#[test]
fn decode_picks_the_format_from_the_first_bytes() {
    let flac = decode(fixture!("s8_quad_8000.flac"), &DecodeLimits::default()).unwrap();
    assert_eq!(flac.channels(), 4);
    let wav = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/wav/s16_stereo_44100.wav"
    ));
    assert_eq!(decode(wav, &DecodeLimits::default()).unwrap().channels(), 2);
    assert!(matches!(
        decode(b"OggS and more", &DecodeLimits::default()),
        Err(DecodeError::NotRecognized | DecodeError::Malformed(_))
    ));
}

#[test]
fn a_frame_that_fails_its_checksum_is_silent() {
    let tone = tone();
    let pcm = decode_default(fixture!("bad_crc_s16_mono_44100.flac")).unwrap();
    let samples = pcm.channel(0);
    assert_eq!(samples.len(), 4096);
    assert_eq!(samples[..1024], tone[..1024]);
    assert!(samples[1024..2048].iter().all(|&s| s == 0.0));
    assert_eq!(samples[2048..], tone[2048..]);
}

/// dr_flac as SoLoud builds it does not check header checksums either; a
/// header that fails is not taken for a frame, and its frame is silent.
#[test]
fn a_frame_header_that_fails_its_checksum_is_silent() {
    let tone = tone();
    let pcm = decode_default(fixture!("bad_crc8_s16_mono_44100.flac")).unwrap();
    let samples = pcm.channel(0);
    assert_eq!(samples.len(), 4096);
    assert_eq!(samples[..2048], tone[..2048]);
    assert!(samples[2048..3072].iter().all(|&s| s == 0.0));
    assert_eq!(samples[3072..], tone[3072..]);
}

#[test]
fn runs_of_damaged_frames_keep_their_place() {
    let pcm = decode_default(fixture!("bad_crc_runs_s16_mono_44100.flac")).unwrap();
    let samples = pcm.channel(0);
    assert_eq!(samples.len(), 6144);
    for (block, good) in [false, false, true, false, false, true]
        .into_iter()
        .enumerate()
    {
        let frames = &samples[block * 1024..][..1024];
        assert_eq!(frames.iter().any(|&s| s != 0.0), good, "block {block}");
    }
}

#[test]
fn a_frame_numbered_before_the_end_of_the_last_adds_nothing_twice() {
    let pcm = decode_default(fixture!("bad_overlap_s16_mono_44100.flac")).unwrap();
    assert_eq!(pcm.frames(), 1024);
}

#[test]
fn data_that_ends_early_keeps_the_frames_present() {
    let tone = tone();
    assert_eq!(tone.len(), 4096);
    let pcm = decode_default(fixture!("short_data_s16_mono_44100.flac")).unwrap();
    assert_eq!(pcm.channel(0), &tone[..3072]);
}

#[test]
fn stereo_data_that_ends_early_keeps_its_planes_apart() {
    let whole = fixture!("s16_stereo_44100.flac");
    let full = decode_default(whole).unwrap();
    let cut = decode_default(&whole[..whole.len() / 2]).unwrap();
    let frames = cut.frames() as usize;
    assert!(frames > 0 && frames < 5108);
    assert_eq!(cut.channel(0), &full.channel(0)[..frames]);
    assert_eq!(cut.channel(1), &full.channel(1)[..frames]);
}

#[test]
fn an_unknown_length_and_bad_headers_are_rejected() {
    assert!(matches!(
        decode_default(fixture!("unknown_length_s16_mono_44100.flac")),
        Err(DecodeError::Unsupported(_))
    ));
    assert!(decode_default(fixture!("bad_cut_header.flac")).is_err());
    assert!(matches!(
        decode_default(fixture!("bad_zero_rate.flac")),
        Err(DecodeError::InvalidSampleRate | DecodeError::Malformed(_))
    ));
    assert!(decode_default(b"fLaC").is_err());
    assert!(decode_default(b"").is_err());
}

/// The first frame's mid and side channels are far beyond 16 bits. The
/// arithmetic wraps in 32 bits as dr_flac's scalar code does, in every
/// build, and shifting the wrapped samples into place leaves nothing of
/// them. (dr_flac's SSE2 code, which x64 runs on most of a 16-bit stereo
/// read, keeps eight more bits, so the C++ output depends on its reads.)
/// The next frame decodes.
#[test]
fn arithmetic_overflow_wraps_as_dr_flac_does() {
    let pcm = decode_default(fixture!("bad_overflow_s16_stereo_44100.flac")).unwrap();
    assert_eq!(pcm.frames(), 8192);
    for channel in 0..2 {
        let samples = pcm.channel(channel);
        assert!(samples[..4096].iter().all(|&s| s == 0.0));
        assert!(samples[4096..].iter().all(|s| (-1.0..1.0).contains(s)));
        assert!(samples[4096..].iter().any(|&s| s != 0.0));
    }
}

#[test]
fn metadata_longer_than_the_file_is_rejected_before_reading_it() {
    // STREAMINFO, then an application block claiming 16 MiB.
    let mut bytes = fixture!("s8_quad_8000.flac")[..42].to_vec();
    bytes[4] &= 0x7F;
    bytes.extend_from_slice(&[0x82, 0xFF, 0xFF, 0xFF, b'T', b'E', b'S', b'T']);
    assert!(matches!(
        decode_default(&bytes),
        Err(DecodeError::Malformed(_))
    ));
}

#[test]
fn the_declared_length_is_checked_against_the_limit() {
    let bytes = fixture!("long_claim_s16_mono_44100.flac");
    let limits = DecodeLimits { max_samples: 9_999 };
    assert!(matches!(
        decode_flac(bytes, &limits),
        Err(DecodeError::TooLarge {
            samples: 10_000,
            ..
        })
    ));
    let exact = DecodeLimits {
        max_samples: 10_000,
    };
    assert!(decode_flac(bytes, &exact).is_ok());
}

#[test]
fn mutated_files_never_panic() {
    let seeds: [&[u8]; 6] = [
        fixture!("s16_stereo_44100.flac"),
        fixture!("s24_mono_48000.flac"),
        fixture!("s8_quad_8000.flac"),
        fixture!("s20_stereo_96000_variable.flac"),
        fixture!("s16_7.1_32000_metadata.flac"),
        fixture!("s32_mono_44100.flac"),
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
    for round in 0..3_000 {
        let mut bytes = seeds[round % seeds.len()].to_vec();
        for _ in 0..1 + next() % 4 {
            // Mostly the metadata and the first frame headers.
            let at = if next() % 4 == 0 {
                next() % bytes.len()
            } else {
                next() % 400.min(bytes.len())
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
        let _ = decode_flac(&bytes, &limits);
    }
}
