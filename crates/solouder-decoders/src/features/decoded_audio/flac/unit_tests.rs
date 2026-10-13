use std::io::Cursor;

use super::bits::{Bits, crc8, crc16};
use super::decorrelate::justify;
use super::frame::{LEFT_SIDE, MID_SIDE, RIGHT_SIDE, coded_number};
use super::subframe::{predict, wide_prediction};
use super::*;

/// Deterministic bytes from a 32-bit LCG.
fn noise(len: usize, mut seed: u32) -> Vec<u8> {
    (0..len)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        })
        .collect()
}

/// The bytes of a string of `0` and `1`, zero-padded to a whole byte.
fn from_bits(text: &str) -> Vec<u8> {
    let bits: Vec<u8> = text
        .bytes()
        .filter(|b| b"01".contains(b))
        .map(|b| b - b'0')
        .collect();
    bits.chunks(8)
        .map(|chunk| (0..8).fold(0, |v, i| (v << 1) | chunk.get(i).copied().unwrap_or(0)))
        .collect()
}

/// Bit `i` of `bytes`, most significant first.
fn bit(bytes: &[u8], i: usize) -> u64 {
    u64::from(bytes[i / 8] >> (7 - i % 8) & 1)
}

#[test]
fn reads_bits_across_refills_and_to_the_end() {
    let bytes = noise(200_000, 7);
    let mut bits = Bits::new(Cursor::new(bytes.clone()));
    let (mut at, mut count) = (0, 0_u32);
    while at + 33 <= bytes.len() * 8 {
        count = count % 33 + 1;
        let expected = (at..at + count as usize).fold(0, |v, i| (v << 1) | bit(&bytes, i));
        assert_eq!(bits.read(count), Some(expected as u32), "at bit {at}");
        at += count as usize;
    }
    let left = (bytes.len() * 8 - at) as u32;
    assert!(bits.read(left).is_some());
    assert_eq!(bits.read(1), None);
}

#[test]
fn signed_reads_extend_the_sign_and_keep_32_bits_of_33() {
    // 1011, then 33 bits: a 0 and 0xFFFFFFF8, of which dr_flac keeps 32.
    let stream = from_bits("1011 0 11111111111111111111111111111000");
    let mut bits = Bits::new(Cursor::new(stream));
    assert_eq!(bits.read_signed(4), Some(-5));
    assert_eq!(bits.read_signed(33), Some(-8));
}

#[test]
fn unary_and_rice_codes() {
    // 100 zeros and a one; Rice codes with parameter 2 of quotient 3,
    // remainder 2 (14, zigzag 7) and quotient 0, remainder 3 (3, zigzag -2).
    let stream = from_bits(&format!("{}1 0001 10 1 11", "0".repeat(100)));
    let mut bits = Bits::new(Cursor::new(stream));
    assert_eq!(bits.unary(), Some(100));
    assert_eq!(bits.rice(2), Some(7));
    assert_eq!(bits.rice(2).map(|v| v as i32), Some(-2));
    assert_eq!(bits.unary(), None);
}

#[test]
fn crcs_match_their_check_values() {
    assert_eq!(crc8(b"123456789"), 0xF4);
    assert_eq!(crc16(b"123456789"), 0xFEE8);
    let bytes = noise(300, 11);
    for len in 0..bytes.len() {
        let bytewise = bytes[..len].iter().fold(0_u16, |mut crc, &b| {
            crc ^= u16::from(b) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 {
                    (crc << 1) ^ 0x8005
                } else {
                    crc << 1
                };
            }
            crc
        });
        assert_eq!(crc16(&bytes[..len]), bytewise, "{len} bytes");
    }
}

#[test]
fn coded_numbers_take_one_to_seven_bytes() {
    let cases: [(&[u8], Option<u64>); 5] = [
        (&[0x7F], Some(0x7F)),
        (&[0xC2, 0x80], Some(0x80)),
        (&[0xEF, 0xBF, 0xBF], Some(0xFFFF)),
        (
            &[0xFE, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF],
            Some(0xF_FFFF_FFFF),
        ),
        (&[0x80], None),
    ];
    for (bytes, expected) in cases {
        let mut bits = Bits::new(Cursor::new(bytes.to_vec()));
        assert_eq!(coded_number(&mut bits), Some(expected), "{bytes:02X?}");
    }
}

#[test]
fn wide_predictions_start_past_32_bits() {
    // Sample bits + precision + bit length of the order.
    assert!(!wide_prediction(16, 1, 15));
    assert!(wide_prediction(17, 1, 15));
    assert!(wide_prediction(16, 32, 12));
    assert!(!wide_prediction(14, 32, 12));
    assert!(!wide_prediction(25, 4, 4));
    assert!(wide_prediction(26, 4, 4));
}

#[test]
fn predictions_sum_in_wrapping_32_or_exact_64_bits() {
    let words = noise(4 * 64 * 200, 3);
    let word = |i: usize| i32::from_le_bytes(words[i * 4..i * 4 + 4].try_into().unwrap());
    for round in 0..200 {
        let order = round % 32 + 1;
        let shift = (round % 16) as u32;
        let base = round * 64;
        let coefficients: Vec<i32> = (0..order).map(|i| word(base + i) >> 17).collect();
        let history: Vec<i32> = (0..order).map(|i| word(base + 32 + i)).collect();
        let exact: i128 = coefficients
            .iter()
            .zip(history.iter().rev())
            .map(|(&c, &s)| i128::from(c) * i128::from(s))
            .sum();
        assert_eq!(
            predict(&coefficients, &history, shift, true),
            (exact >> shift) as i32
        );
        let wrapped = exact as i32;
        assert_eq!(
            predict(&coefficients, &history, shift, false),
            wrapped >> shift
        );
    }
}

#[test]
fn stereo_decorrelation_follows_dr_flac() {
    let stride = 4;
    let run = |assignment, a: [i32; 2], b: [i32; 2], wasted: [u32; 2], unused| {
        let mut planes = vec![a[0], a[1], 0, 0, b[0], b[1], 0, 0];
        justify(assignment, &mut planes, stride, 2, &wasted, unused);
        [planes[0], planes[1], planes[4], planes[5]]
    };
    // 16-bit left/side: right = left - side, left-justified.
    assert_eq!(
        run(LEFT_SIDE, [1000, -5], [1500, 7], [0, 0], 16),
        [1000 << 16, -5 << 16, -500 << 16, -12 << 16]
    );
    // Right/side: left = right + side; wasted bits shift first.
    assert_eq!(
        run(RIGHT_SIDE, [3, 0], [-2, 1], [1, 0], 16),
        [4 << 16, 1 << 16, -2 << 16, 1 << 16]
    );
    // Mid/side: mid 4, side 3 gives left 6 and right 3.
    assert_eq!(
        run(MID_SIDE, [4, -1], [3, -1], [0, 0], 16),
        [6 << 16, -1 << 16, 3 << 16, 0]
    );
    // 32-bit left/side wraps in 32 bits, as dr_flac does.
    assert_eq!(
        run(LEFT_SIDE, [i32::MAX, 0], [-1, 0], [0, 0], 0),
        [i32::MAX, 0, i32::MIN, 0]
    );
}

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

#[test]
fn rewinding_decodes_the_same_frames_again() {
    let mut file = FlacFile::open(Cursor::new(fixture!("s16_stereo_44100.flac"))).unwrap();
    let mut first = Vec::new();
    for _ in 0..3 {
        assert!(file.next_frame(|gap, planes| first.push((gap, planes[1].to_vec()))));
    }
    assert!(file.rewind());
    for (gap, plane) in first {
        assert!(file.next_frame(|g, planes| assert_eq!((g, planes[1]), (gap, &plane[..]))));
    }
}
