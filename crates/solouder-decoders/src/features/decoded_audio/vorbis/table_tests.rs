//! The tables and the inverse MDCT, which need exact C behaviour.

use super::imdct::inverse_mdct;
use super::setup::BlockTables;
use super::tables::{
    compute_bitreverse, compute_twiddle_factors, compute_window, crc32_update, float32_unpack,
    ilog, lookup1_values,
};

#[test]
fn crc_matches_the_ogg_polynomial() {
    let crc = b"123456789".iter().fold(0, |crc, &b| crc32_update(crc, b));
    assert_eq!(crc, 0x89A1_897F);
}

#[test]
fn ilog_counts_bits() {
    let cases = [
        (-1, 0),
        (0, 0),
        (1, 1),
        (2, 2),
        (3, 2),
        (4, 3),
        (7, 3),
        (1 << 30, 31),
    ];
    for (n, bits) in cases {
        assert_eq!(ilog(n), bits, "{n}");
    }
    assert_eq!(ilog(i32::MAX), 31);
}

#[test]
fn float32_unpack_follows_the_vorbis_format() {
    // mantissa 1, exponent 788 - 788 = 0
    assert_eq!(float32_unpack(788 << 21 | 1), 1.0);
    assert_eq!(float32_unpack(0x8000_0000 | 788 << 21 | 3), -3.0);
    assert_eq!(float32_unpack(790 << 21 | 5), 20.0);
    assert_eq!(float32_unpack(0), 0.0);
    assert!(float32_unpack(u32::MAX).is_infinite());
}

#[test]
fn lookup1_values_keeps_msvc_conversions() {
    assert_eq!(lookup1_values(81, 4), 3);
    assert_eq!(lookup1_values(80, 4), 2);
    assert_eq!(lookup1_values(1, 1), 1);
    // 2^40 does not fit an int: MSVC converts it to INT_MIN, which passes
    // `<= entries`, so r goes from 1 to 2.
    assert_eq!(lookup1_values(1, 40), 2);
}

/// The tables of blocksize `n`, as `start_decoder` builds them.
fn block_tables(n: usize) -> BlockTables {
    let mut t = BlockTables {
        a: vec![0.0; n / 2],
        b: vec![0.0; n / 2],
        c: vec![0.0; n / 4],
        window: vec![0.0; n / 2],
        bit_reverse: vec![0; n / 8],
    };
    compute_twiddle_factors(n as i32, &mut t.a, &mut t.b, &mut t.c);
    compute_window(n as i32, &mut t.window);
    compute_bitreverse(n as i32, &mut t.bit_reverse);
    t
}

/// stb's `inverse_mdct` of `input` (the first `n / 2` values).
fn imdct(input: &[f32]) -> Vec<f32> {
    let n = input.len() * 2;
    let mut buffer = input.to_vec();
    buffer.resize(n, 0.0);
    inverse_mdct(&mut buffer, n, &block_tables(n), &mut vec![0.0; n / 2]);
    buffer
}

#[test]
fn inverse_mdct_matches_the_textbook_transform_from_256() {
    let mut seed = 1_u32;
    for bits in 8..=13 {
        let n = 1 << bits;
        let input: Vec<f32> = (0..n / 2)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
            })
            .collect();
        let pi = std::f64::consts::PI;
        for (i, &x) in imdct(&input).iter().enumerate() {
            let phase = 2.0 * pi / n as f64 * (i as f64 + 0.5 + n as f64 / 4.0);
            let y = input.iter().enumerate().fold(0.0, |y, (k, &v)| {
                y + f64::from(v) * (phase * (k as f64 + 0.5)).cos()
            });
            assert!((f64::from(x) - y).abs() < 1e-3, "{n} {i}: {x} {y}");
        }
    }
}

#[test]
fn inverse_mdct_keeps_the_c_result_below_256() {
    // stb's step 3 runs iterations 0 and 1, then its pass for ld-6 to ld-4,
    // which repeats them when n is 64 or 128: the result is no IMDCT, but
    // it is the C's (bits from stb_vorbis.c built with MSVC).
    let mut impulse = vec![0.0; 32];
    impulse[0] = 1.0;
    let bits: Vec<u32> = imdct(&impulse)[..4].iter().map(|x| x.to_bits()).collect();
    assert_eq!(bits, [0x3fbd_2b66, 0x3fc8_9ffe, 0xbebd_76ac, 0xbed7_cc86]);
}
