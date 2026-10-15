//! The fixtures, the file path against the memory path, seeking against
//! decoding straight through, and SoLoud's two kinds of `File`.

use std::io::Cursor;

use super::decoder::Vorbis;
use super::source::{FileSource, Memory, SoloudFile, Source};
use super::*;

/// A file from `tests/fixtures/ogg`.
pub(super) fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../tests/fixtures/ogg/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The fixtures that open, with their channels, frames and sample rate.
pub(super) const GOOD: [(&str, u32, u32, f32); 9] = [
    ("vorbis_stereo_44100.ogg", 2, 66150, 44100.0),
    ("vorbis_mono_22050_q0.ogg", 1, 22050, 22050.0),
    ("vorbis_5.1_48000.ogg", 6, 24000, 48000.0),
    ("vorbis_7.1_32000.ogg", 8, 12800, 32000.0),
    ("vorbis_quad_16000_q10.ogg", 4, 9600, 16000.0),
    ("vorbis_mono_8000_long.ogg", 1, 40000, 8000.0),
    ("vorbis_short_mono_44100.ogg", 1, 132, 44100.0),
    ("bad_payload_stereo_44100.ogg", 2, 66150, 44100.0),
    ("bad_crc_stereo_44100.ogg", 2, 66150, 44100.0),
];

/// Frames: where stb says each starts, and each channel's samples.
pub(super) type Frames = Vec<(Option<u32>, Vec<Vec<f32>>)>;

/// Every frame `file` returns from here on (at most `limit`).
pub(super) fn frames_of<R: Read + Seek>(file: &mut VorbisFile<R>, limit: usize) -> Frames {
    let mut frames = Vec::new();
    for _ in 0..limit {
        let at = file.sample_offset();
        if file.get_frame_float() == 0 {
            break;
        }
        let channels = (0..file.channels() as usize).map(|c| file.output(c).to_vec());
        frames.push((at, channels.collect()));
    }
    frames
}

/// Every frame of `bytes` through stb's memory path.
fn memory_frames(bytes: &[u8]) -> Frames {
    let mut v = Vorbis::open(Memory::new(bytes), bytes.len() as u32).unwrap();
    let mut frames = Vec::new();
    loop {
        let at = u32::try_from(v.sample_offset()).ok();
        let (start, n) = v.get_frame_float();
        if n == 0 {
            return frames;
        }
        let stride = v.stride();
        let channels = (0..v.s.channels as usize)
            .map(|c| v.channel_buffers[c * stride + start..][..n].to_vec());
        frames.push((at, channels.collect()));
    }
}

/// Channel `c` of `frames` end to end.
fn joined(frames: &Frames, c: usize) -> Vec<f32> {
    frames
        .iter()
        .flat_map(|(_, ch)| ch[c].iter().copied())
        .collect()
}

#[test]
fn fixtures_decode_with_their_layout() {
    for (name, channels, frames, rate) in GOOD {
        let pcm = decode_ogg(&fixture(name), &DecodeLimits::default()).unwrap();
        assert_eq!(
            (pcm.channels(), pcm.frames(), pcm.sample_rate()),
            (channels, frames, rate)
        );
        assert!(pcm.channel(0).iter().all(|s| s.is_finite()), "{name}");
        assert!(pcm.channel(0).iter().any(|&s| s != 0.0), "{name}");
    }
}

#[test]
fn broken_fixtures_fail_to_open() {
    let broken = [
        "bad_cut_header.ogg",
        "bad_headers_only.ogg",
        "bad_setup.ogg",
        "bad_truncated_stereo_44100.ogg",
    ];
    for name in broken {
        let result = decode_ogg(&fixture(name), &DecodeLimits::default());
        assert!(
            matches!(result, Err(DecodeError::Malformed(_))),
            "{name}: {result:?}"
        );
    }
    assert!(matches!(
        decode_ogg(b"", &DecodeLimits::default()),
        Err(DecodeError::NotRecognized)
    ));
    let wav = b"RIFF\0\0\0\0WAVEfmt ";
    assert!(matches!(
        decode_ogg(wav, &DecodeLimits::default()),
        Err(DecodeError::NotRecognized)
    ));
}

#[test]
fn the_limit_is_checked_before_decoding() {
    let bytes = fixture("vorbis_stereo_44100.ogg");
    let limits = DecodeLimits {
        max_samples: 2 * 66150 - 1,
    };
    let result = decode_ogg(&bytes, &limits);
    assert!(
        matches!(
            result,
            Err(DecodeError::TooLarge {
                samples: 132_300,
                ..
            })
        ),
        "{result:?}"
    );
    let limits = DecodeLimits {
        max_samples: 2 * 66150,
    };
    assert_eq!(decode_ogg(&bytes, &limits).unwrap().frames(), 66150);
}

#[test]
fn the_file_paths_decode_like_the_memory_path() {
    for (name, channels, frames, _) in GOOD {
        let bytes = fixture(name);
        let expected = memory_frames(&bytes);
        let pcm = decode_ogg(&bytes, &DecodeLimits::default()).unwrap();
        // frames stop at the first that fails (bad_payload); silence follows
        for c in 0..channels as usize {
            let all = joined(&expected, c);
            let (decoded, rest) = pcm
                .channel(c as u32)
                .split_at(all.len().min(frames as usize));
            assert!(
                all.starts_with(decoded) && rest.iter().all(|&s| s == 0.0),
                "{name}"
            );
        }
        for kind in [FileKind::Disk, FileKind::Memory] {
            let mut file = VorbisFile::open_as(Cursor::new(&bytes), kind).unwrap();
            assert_eq!(
                (file.channels(), file.sample_rate() as f32),
                (channels, pcm.sample_rate())
            );
            assert_eq!(file.length_in_samples(), frames, "{name} {kind:?}");
            assert!(
                frames_of(&mut file, usize::MAX) == expected,
                "{name} {kind:?}"
            );
        }
    }
}

#[test]
fn a_stream_may_start_inside_the_reader() {
    let bytes = fixture("vorbis_quad_16000_q10.ogg");
    let mut padded = vec![0xAA; 1000];
    padded.extend_from_slice(&bytes);
    let mut cursor = Cursor::new(padded);
    cursor.set_position(1000);
    let mut file = VorbisFile::open_as(cursor, FileKind::Disk).unwrap();
    assert_eq!(file.length_in_samples(), 9600);
    assert!(frames_of(&mut file, usize::MAX) == memory_frames(&bytes));
}

#[test]
fn seek_start_restarts_identically() {
    for (name, ..) in GOOD {
        let mut file = VorbisFile::open_as(Cursor::new(fixture(name)), FileKind::Disk).unwrap();
        let first = frames_of(&mut file, usize::MAX);
        for limit in [usize::MAX, 3, 1] {
            assert!(file.seek_start(), "{name}");
            assert!(frames_of(&mut file, usize::MAX) == first, "{name}");
            assert!(file.seek_start());
            frames_of(&mut file, limit);
        }
    }
}

#[test]
fn seek_frame_lands_on_the_frame_holding_the_sample() {
    for (name, channels, frames, _) in GOOD {
        let bytes = fixture(name);
        for kind in [FileKind::Disk, FileKind::Memory] {
            let mut file = VorbisFile::open_as(Cursor::new(&bytes), kind).unwrap();
            let all: Vec<Vec<f32>> = {
                let straight = frames_of(&mut file, usize::MAX);
                (0..channels as usize)
                    .map(|c| joined(&straight, c))
                    .collect()
            };
            let step = (frames / 37).max(1);
            let targets = (0..frames)
                .step_by(step as usize)
                .chain([1, frames / 2, frames - 1]);
            for target in targets {
                // around bad_payload's damaged packet, seeks may fail and
                // frames differ from those decoded straight through
                let damaged = name.starts_with("bad_");
                if !file.seek_frame(target) {
                    assert!(damaged, "{name} {kind:?} {target}");
                    continue;
                }
                let at = file.sample_offset().unwrap();
                let n = file.get_frame_float();
                assert!(
                    at <= target && target < at + n as u32,
                    "{name} {target}: {at}+{n}"
                );
                for (c, all) in all.iter().enumerate().filter(|_| !damaged) {
                    let expected = &all[at as usize..at as usize + n];
                    assert!(file.output(c) == expected, "{name} {kind:?} {target} ch{c}");
                }
            }
            assert!(
                !file.seek_frame(frames + 1),
                "{name} {kind:?}: past the end"
            );
            assert!(file.seek_frame(0) && file.sample_offset() == Some(0));
        }
    }
}

/// `Source` over `bytes` through a SoLoud `File` of `kind`.
fn file_source(bytes: &[u8], kind: FileKind) -> (FileSource<Cursor<&[u8]>>, u32) {
    FileSource::open(SoloudFile::new(Cursor::new(bytes), kind).unwrap())
}

#[test]
fn a_memory_file_measures_one_byte_short() {
    assert_eq!(file_source(b"abc", FileKind::Disk).1, 3);
    assert_eq!(file_source(b"abc", FileKind::Memory).1, 2);
    let (mut f, _) = file_source(b"abc", FileKind::Memory);
    // seeks stop at the last byte
    assert_eq!(f.set_offset(10), (true, false));
    assert_eq!((f.offset(), f.get8(), f.get8()), (2, Some(b'c'), None));
}

#[test]
fn a_disk_file_reads_a_zero_before_its_end() {
    let (mut f, _) = file_source(b"abc", FileKind::Disk);
    let bytes: Vec<_> = (0..5).map(|_| f.get8()).collect();
    assert_eq!(bytes, [Some(b'a'), Some(b'b'), Some(b'c'), Some(0), None]);
    assert_eq!(f.set_offset(1), (true, false));
    assert_eq!(f.get8(), Some(b'b'));
    // seeks may pass the end
    assert_eq!(f.set_offset(10), (true, false));
    assert_eq!((f.offset(), f.get8(), f.get8()), (10, Some(0), None));
    assert_eq!(f.set_offset(0x8000_0000), (true, true));
}

#[test]
fn empty_and_short_reads_fail() {
    for kind in [FileKind::Disk, FileKind::Memory] {
        let (mut f, _) = file_source(b"abc", kind);
        assert!(!f.getn(&mut []), "{kind:?}");
        assert!(!f.getn(&mut [0; 4]), "{kind:?}");
    }
    let mut m = Memory::new(b"abc");
    assert!(m.getn(&mut []) && !m.getn(&mut [0; 4]));
    assert!(m.skip(5) && m.get8().is_none());
}

#[test]
fn files_that_cannot_be_read_fail_to_open() {
    for kind in [FileKind::Disk, FileKind::Memory] {
        let result = VorbisFile::open_as(Cursor::new(Vec::new()), kind);
        assert!(
            matches!(result, Err(DecodeError::NotRecognized)),
            "{kind:?}: {:?}",
            result.err()
        );
    }
    let mut cursor = Cursor::new(fixture("vorbis_short_mono_44100.ogg"));
    cursor.set_position(1 << 20);
    assert!(matches!(
        VorbisFile::open_as(cursor, FileKind::Disk),
        Err(DecodeError::Io(_))
    ));
}
