use std::io::{self, Cursor, Read, Seek, SeekFrom};

use super::*;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/mp3/",
            $name
        ))
        .as_slice()
    };
}

/// A fixture that decodes: name, bytes, channels, sample rate, frames (as
/// dr_mp3 counts them) and an FNV-1a hash of the samples' bits, checked
/// against dr_mp3 bit for bit.
type Fixture = (&'static str, &'static [u8], u32, u32, u32, u64);

pub(super) fn fixtures() -> Vec<Fixture> {
    vec![
        (
            "cbr128",
            fixture!("mpeg1_stereo_44100_cbr128.mp3"),
            2,
            44100,
            69120,
            0xBC61_B820_B05C_38F5,
        ),
        (
            "vbr_id3",
            fixture!("mpeg1_stereo_48000_vbr_id3.mp3"),
            2,
            48000,
            50688,
            0x990A_52B2_9F83_A04C,
        ),
        (
            "noxing",
            fixture!("mpeg1_mono_32000_noxing.mp3"),
            1,
            32000,
            27648,
            0xBA22_FE7D_5F06_EC0C,
        ),
        (
            "mpeg2_vbr",
            fixture!("mpeg2_mono_22050_vbr.mp3"),
            1,
            22050,
            24192,
            0x97EC_599D_1ECD_B1F3,
        ),
        (
            "mpeg2_st",
            fixture!("mpeg2_stereo_16000.mp3"),
            2,
            16000,
            13248,
            0x56E6_CEBC_BFC8_9CCE,
        ),
        (
            "mpeg25_m",
            fixture!("mpeg25_mono_8000.mp3"),
            1,
            8000,
            9792,
            0x8058_186F_0202_28DA,
        ),
        (
            "mpeg25_st",
            fixture!("mpeg25_stereo_11025_long.mp3"),
            2,
            11025,
            57024,
            0xC637_15E6_3B77_272F,
        ),
        (
            "layer2_st",
            fixture!("layer2_stereo_44100.mp3"),
            2,
            44100,
            44928,
            0xCB59_87DF_8E49_43D3,
        ),
        (
            "layer2_m",
            fixture!("layer2_mono_24000.mp3"),
            1,
            24000,
            14976,
            0xE32A_C6D3_BC63_0794,
        ),
        (
            "truncated",
            fixture!("bad_truncated_stereo_44100.mp3"),
            2,
            44100,
            41472,
            0x7A03_D28A_CC6A_777B,
        ),
        (
            "junk",
            fixture!("bad_junk_prefix_stereo_44100.mp3"),
            2,
            44100,
            69120,
            0xBC61_B820_B05C_38F5,
        ),
        (
            "damaged",
            fixture!("bad_payload_stereo_44100.mp3"),
            2,
            44100,
            64512,
            0x06B3_CA40_2F79_0BDE,
        ),
    ]
}

fn hash(samples: impl IntoIterator<Item = f32>) -> u64 {
    samples.into_iter().fold(0xcbf2_9ce4_8422_2325, |h, s| {
        s.to_bits()
            .to_le_bytes()
            .iter()
            .fold(h, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
    })
}

fn interleaved(pcm: &Pcm) -> Vec<f32> {
    let (channels, frames) = (pcm.channels(), pcm.frames() as usize);
    (0..frames)
        .flat_map(|i| (0..channels).map(move |c| pcm.channel(c)[i]))
        .collect()
}

#[test]
fn decodes_the_fixtures_as_dr_mp3_does() {
    let mut wrong = Vec::new();
    for (name, bytes, channels, rate, frames, expected) in fixtures() {
        let pcm = decode_mp3(bytes, &DecodeLimits::default()).unwrap();
        assert_eq!(pcm.channels(), channels, "{name}");
        assert_eq!(pcm.sample_rate(), rate as f32, "{name}");
        assert_eq!(pcm.frames(), frames, "{name}");
        let got = hash((0..channels).flat_map(|c| pcm.channel(c).iter().copied()));
        if got != expected {
            wrong.push(format!("{name}: {got:#018X}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn files_without_audio_and_over_the_limit_are_refused() {
    let limits = DecodeLimits::default();
    let none = decode_mp3(fixture!("bad_not_audio.mp3"), &limits);
    assert!(matches!(none, Err(DecodeError::NotRecognized)));
    assert!(matches!(
        decode_mp3(&[], &limits),
        Err(DecodeError::NotRecognized)
    ));
    let small = DecodeLimits { max_samples: 1000 };
    let over = decode_mp3(fixture!("mpeg2_stereo_16000.mp3"), &small);
    assert!(matches!(
        over,
        Err(DecodeError::TooLarge {
            samples: 26496,
            limit: 1000
        })
    ));
}

/// Reads up to `limit` frames from `file` in reads of `chunk` frames.
pub(super) fn read_stream<R: Read + Seek>(
    file: &mut Mp3File<R>,
    chunk: usize,
    limit: usize,
) -> Vec<f32> {
    let channels = file.channels() as usize;
    let mut out = Vec::new();
    let mut buf = vec![0.0; chunk * channels];
    while out.len() < limit.saturating_mul(channels) {
        let want = chunk.min(limit - out.len() / channels);
        let read = file.read_f32(&mut buf, want);
        out.extend_from_slice(&buf[..read * channels]);
        if read < want {
            break;
        }
    }
    out
}

#[test]
fn streams_decode_as_memory_does() {
    for (name, bytes, channels, rate, frames, _) in fixtures() {
        let pcm = interleaved(&decode_mp3(bytes, &DecodeLimits::default()).unwrap());
        let mut file = Mp3File::open(Cursor::new(bytes)).unwrap();
        assert_eq!(
            (file.channels(), file.sample_rate()),
            (channels, rate),
            "{name}"
        );
        assert_eq!(file.frame_count(), u64::from(frames), "{name}");
        let streamed = read_stream(&mut file, 333, usize::MAX);
        // The count may pass what decodes; `Wav` leaves the rest silent.
        assert_eq!(streamed[..], pcm[..streamed.len()], "{name}");
        assert!(pcm[streamed.len()..].iter().all(|&s| s == 0.0), "{name}");
        if name != "damaged" {
            assert_eq!(streamed.len(), pcm.len(), "{name}: frame count");
        }
    }
}

#[test]
fn rewinding_and_counting_keep_the_stream_where_it_was() {
    for (name, bytes, channels, ..) in fixtures() {
        let mut file = Mp3File::open(Cursor::new(bytes)).unwrap();
        let whole = read_stream(&mut file, 512, usize::MAX);
        assert!(file.rewind(), "{name}");
        assert_eq!(read_stream(&mut file, 1000, usize::MAX), whole, "{name}");
        assert!(file.rewind(), "{name}");
        let start = read_stream(&mut file, 4096, 3000);
        assert_eq!(start[..], whole[..start.len()], "{name}");

        let mut file = Mp3File::open(Cursor::new(bytes)).unwrap();
        let mut parts = read_stream(&mut file, 100, 1234);
        assert_eq!(parts.len(), 1234 * channels as usize, "{name}");
        file.frame_count();
        parts.extend(read_stream(&mut file, 777, usize::MAX));
        assert_eq!(parts, whole, "{name}");
    }
}

/// A reader that fails from byte `fail_at` on, reads at most 100 bytes at
/// a time, and may refuse to seek.
struct Failing<'a> {
    inner: Cursor<&'a [u8]>,
    fail_at: u64,
    seeks: bool,
}

impl Read for Failing<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.fail_at.saturating_sub(self.inner.position());
        if left == 0 {
            return Err(io::Error::other("broken"));
        }
        let n = buf.len().min(left as usize).min(100);
        self.inner.read(&mut buf[..n])
    }
}

impl Seek for Failing<'_> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        if self.seeks {
            self.inner.seek(to)
        } else {
            Err(io::Error::other("no seeking"))
        }
    }
}

#[test]
fn read_errors_end_the_data_as_a_short_file_does() {
    let bytes = fixture!("mpeg1_stereo_44100_cbr128.mp3");
    for fail_at in [3000, 10_000, 17_777] {
        let inner = Cursor::new(bytes);
        let mut file = Mp3File::open(Failing {
            inner,
            fail_at,
            seeks: true,
        })
        .unwrap();
        let mut short = Mp3File::open(Cursor::new(&bytes[..fail_at as usize])).unwrap();
        assert_eq!(file.frame_count(), short.frame_count());
        let expected = read_stream(&mut short, 512, usize::MAX);
        assert_eq!(read_stream(&mut file, 512, usize::MAX), expected);
    }
    let inner = Cursor::new(bytes);
    let mut file = Mp3File::open(Failing {
        inner,
        fail_at: u64::MAX,
        seeks: false,
    })
    .unwrap();
    assert_eq!(file.frame_count(), 0);
    assert!(!file.rewind());
    assert_eq!(read_stream(&mut file, 512, 100).len(), 200);
}

#[test]
fn a_synthetic_layer_1_stream_decodes() {
    // MPEG-1 layer 1, 384 kbps, 48 kHz, stereo: 384-byte frames whose
    // allocations give every band 2 bits per sample, then noise.
    let mut file = Vec::new();
    let mut seed = 7_u32;
    for _ in 0..20 {
        file.extend_from_slice(&[0xFF, 0xFF, 0xC4, 0x00]);
        file.extend(std::iter::repeat_n(0x11, 32));
        for _ in 0..384 - 4 - 32 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            file.push((seed >> 24) as u8);
        }
    }
    let pcm = decode_mp3(&file, &DecodeLimits::default()).unwrap();
    assert_eq!((pcm.channels(), pcm.frames()), (2, 20 * 384));
    assert!(pcm.channel(0).iter().any(|&s| s != 0.0));
}

#[path = "fuzz_tests.rs"]
mod fuzz_tests;
#[path = "table_tests.rs"]
mod table_tests;
