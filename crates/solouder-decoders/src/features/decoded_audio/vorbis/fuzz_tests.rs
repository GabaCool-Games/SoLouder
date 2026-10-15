//! Damaged files: no input may panic, in debug builds included (where
//! integer overflow and out-of-bounds indexing would), through
//! `decode_ogg` and through `VorbisFile` with seeks.

use std::io::Cursor;

use super::tables::crc32_update;
use super::unit_tests::{GOOD, fixture, frames_of};
use super::*;

/// xorshift32.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { self.next() as usize % n }
    }
}

/// Where the setup header ends in every fixture, roughly: the part whose
/// damage reaches the most code.
const HEADERS: usize = 4400;

/// A damaged copy of `bytes`, often damaged only in its headers.
fn mutate(bytes: &[u8], other: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut b = bytes.to_vec();
    let headers_only = rng.below(2) == 0;
    for _ in 0..1 + rng.below(4) {
        let region = if headers_only {
            HEADERS.min(b.len())
        } else {
            b.len()
        };
        let at = rng.below(region);
        let len = 1 + rng.below(64);
        match rng.below(9) {
            0..=2 => {
                for _ in 0..1 + rng.below(8) {
                    let i = rng.below(region);
                    b[i] ^= 1 << rng.below(8);
                }
            }
            3 => b[at] = [0, 0xFF, 0x7F, 0x80, rng.next() as u8][rng.below(5)],
            4 => {
                let end = (at + len).min(b.len());
                b[at..end].iter_mut().for_each(|x| *x = rng.next() as u8);
            }
            5 => {
                b.drain(at..(at + len).min(b.len()));
            }
            6 => {
                let chunk = b[at..(at + len).min(b.len())].to_vec();
                let to = rng.below(b.len());
                b.splice(to..to, chunk);
            }
            7 => b.truncate(at),
            _ => {
                // the tail of another file
                let from = rng.below(other.len());
                b.truncate(at);
                b.extend_from_slice(&other[from..]);
            }
        }
        if b.is_empty() {
            break;
        }
    }
    b
}

/// Everything `VorbisFile` can be asked, on `bytes`.
fn exercise(bytes: &[u8], kind: FileKind, rng: &mut Rng) {
    let Ok(mut file) = VorbisFile::open_as(Cursor::new(bytes), kind) else {
        return;
    };
    assert!((1..=16).contains(&file.channels()) && file.sample_rate() != 0);
    let len = file.length_in_samples();
    frames_of(&mut file, 40);
    let targets = [
        0,
        len.wrapping_sub(1),
        len,
        u32::MAX,
        rng.next() % len.max(1),
        rng.next() % len.max(1),
    ];
    for target in targets {
        if file.seek_frame(target) {
            let at = file.sample_offset();
            assert!(at.is_some_and(|at| at <= target), "{target} {at:?}");
        }
        frames_of(&mut file, 3);
    }
    if file.seek_start() {
        frames_of(&mut file, 20);
    }
    for c in 0..17 {
        let _ = file.output(c);
    }
}

#[test]
fn mutated_files_never_panic() {
    let files: Vec<Vec<u8>> = GOOD.iter().map(|(name, ..)| fixture(name)).collect();
    let limits = DecodeLimits::default();
    let mut rng = Rng(0x2545_F491);
    for i in 0..3000 {
        let source = &files[i % files.len()];
        let other = &files[rng.below(files.len())];
        let bytes = mutate(source, other, &mut rng);
        if let Ok(pcm) = decode_ogg(&bytes, &limits) {
            assert!(pcm.channels() <= MAX_CHANNELS && pcm.frames() > 0);
        }
        if i % 3 == 0 {
            let kind = if i % 2 == 0 {
                FileKind::Disk
            } else {
                FileKind::Memory
            };
            exercise(&bytes, kind, &mut rng);
        }
    }
}

#[test]
fn every_truncation_fails_or_decodes() {
    let limits = DecodeLimits::default();
    let mut rng = Rng(7);
    let short = fixture("vorbis_short_mono_44100.ogg");
    for len in 0..short.len() {
        let _ = decode_ogg(&short[..len], &limits);
        let kind = if len % 2 == 0 {
            FileKind::Disk
        } else {
            FileKind::Memory
        };
        exercise(&short[..len], kind, &mut rng);
    }
    // the headers of a larger file, then one cut every 97 bytes
    let stereo = fixture("vorbis_stereo_44100.ogg");
    let cuts = (0..HEADERS).chain((HEADERS..stereo.len()).step_by(97));
    for len in cuts {
        let _ = decode_ogg(&stereo[..len], &limits);
    }
}

#[test]
fn absurd_sizes_are_typed_errors() {
    let bytes = fixture("vorbis_stereo_44100.ogg");
    let limits = DecodeLimits { max_samples: 0 };
    assert!(matches!(
        decode_ogg(&bytes, &limits),
        Err(DecodeError::TooLarge { .. })
    ));
    // the last page claims the largest length stb reports, 2^32 - 2 frames
    // (2^32 - 1 is its "unknown"), or more (which it saturates to that)
    for granule in [0xFFFF_FFFE_u64, 1 << 40] {
        let result = decode_ogg(
            &with_last_granule(&bytes, granule),
            &DecodeLimits {
                max_samples: u64::MAX,
            },
        );
        let samples = 2 * 0xFFFF_FFFE;
        assert!(matches!(result, Err(DecodeError::TooLarge { samples: s, .. }) if s == samples));
    }
    let result = decode_ogg(
        &with_last_granule(&bytes, u64::MAX),
        &DecodeLimits::default(),
    );
    assert!(matches!(result, Err(DecodeError::Malformed(_))));
}

/// `bytes` with `granule` in its last page, and the CRC to match (without
/// which stb takes the page before as the last).
fn with_last_granule(bytes: &[u8], granule: u64) -> Vec<u8> {
    let mut huge = bytes.to_vec();
    let last = huge.windows(4).rposition(|w| w == b"OggS").unwrap();
    huge[last + 6..last + 14].copy_from_slice(&granule.to_le_bytes());
    huge[last + 22..last + 26].fill(0);
    let crc = huge[last..].iter().fold(0, |crc, &b| crc32_update(crc, b));
    huge[last + 22..last + 26].copy_from_slice(&crc.to_le_bytes());
    huge
}
