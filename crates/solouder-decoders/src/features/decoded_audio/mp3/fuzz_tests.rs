//! Damaged files: no input may panic, in debug builds included (where
//! integer overflow and out-of-bounds indexing would).

use std::io::Cursor;

use super::super::*;
use super::{fixtures, read_stream};

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

/// A damaged copy of (part of) `bytes`.
fn mutate(bytes: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut b = if rng.below(4) == 0 {
        bytes.to_vec()
    } else {
        // A window, so that most cases stay small.
        let start = rng.below(bytes.len());
        let len = rng.below(6000);
        bytes[start..(start + len).min(bytes.len())].to_vec()
    };
    if b.is_empty() {
        return b;
    }
    let syncs: Vec<usize> = (0..b.len().saturating_sub(4))
        .filter(|&i| b[i] == 0xFF && b[i + 1] & 0xE0 == 0xE0)
        .collect();
    for _ in 0..1 + rng.below(4) {
        match rng.below(7) {
            0 => {
                let i = rng.below(b.len());
                b[i] ^= 1 << rng.below(8);
            }
            1 => {
                for _ in 0..1 + rng.below(40) {
                    let i = rng.below(b.len());
                    b[i] = rng.next() as u8;
                }
            }
            2 => b.truncate(rng.below(b.len())),
            3 => {
                let i = rng.below(b.len());
                let n = rng.below(500).min(b.len() - i);
                b.drain(i..i + n);
            }
            4 if !syncs.is_empty() => {
                // A header or the side information after it: layers, rates,
                // free format, modes, block types, scale factor selection.
                let at = syncs[rng.below(syncs.len())] + 1 + rng.below(12);
                if at < b.len() {
                    b[at] = rng.next() as u8;
                }
            }
            5 => {
                // Valid-looking headers over noise.
                let i = rng.below(b.len());
                let h = [
                    0xFF,
                    0xE2 | (rng.next() as u8 & 0x1F),
                    rng.next() as u8,
                    rng.next() as u8,
                ];
                for (k, &v) in h.iter().enumerate() {
                    if let Some(x) = b.get_mut(i + k) {
                        *x = v;
                    }
                }
            }
            _ => {
                let i = rng.below(b.len());
                let chunk: Vec<u8> = b[i..].iter().take(rng.below(3000)).copied().collect();
                let at = rng.below(b.len());
                b.splice(at..at, chunk);
            }
        }
        if b.is_empty() {
            break;
        }
    }
    b
}

fn exercise(bytes: &[u8]) {
    let _ = decode_mp3(bytes, &DecodeLimits::default());
    if let Ok(mut file) = Mp3File::open(Cursor::new(bytes)) {
        file.frame_count();
        read_stream(&mut file, 1000, 1 << 20);
        file.rewind();
        read_stream(&mut file, 512, 2000);
    }
}

#[test]
fn damaged_files_never_panic() {
    let mut rng = Rng(0x9E37_79B9);
    let fixtures = fixtures();
    let cases = if cfg!(debug_assertions) { 3000 } else { 20000 };
    for case in 0..cases {
        let (_, bytes, ..) = fixtures[case % fixtures.len()];
        exercise(&mutate(bytes, &mut rng));
    }
}

#[test]
fn noise_and_tiny_inputs_never_panic() {
    let mut rng = Rng(12345);
    for len in 0..64 {
        for _ in 0..20 {
            let bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            exercise(&bytes);
            // Headers only.
            let mut headers = bytes.clone();
            for chunk in headers.chunks_mut(4) {
                chunk[0] = 0xFF;
            }
            exercise(&headers);
        }
    }
    for _ in 0..50 {
        let bytes: Vec<u8> = (0..70_000).map(|_| rng.next() as u8 | 0xE0).collect();
        exercise(&bytes);
    }
}
