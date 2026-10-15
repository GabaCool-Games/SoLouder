// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! Layer 3 side information and scale factors (minimp3's
//! `drmp3_L3_read_side_info`, `drmp3_L3_read_scalefactors`,
//! `drmp3_L3_ldexp_q2` and `drmp3_L3_decode_scalefactors`).

// Float constants keep C's digits, so they round to the same `f32`.
#![allow(clippy::excessive_precision)]

use super::bits::Bs;
use super::header::{get_my_sample_rate, is_mono, is_ms_stereo, test_i_stereo, test_mpeg1};
use super::tables::{SCF_LONG, SCF_MIXED, SCF_SHORT};

const SHORT_BLOCK_TYPE: u8 = 2;
/// `DRMP3_BITS_DEQUANTIZER_OUT`.
const BITS_DEQUANTIZER_OUT: i32 = -1;
/// `DRMP3_MAX_SCFI`.
const MAX_SCFI: i32 = ((255 + BITS_DEQUANTIZER_OUT * 4 - 210) + 3) & !3;

/// `drmp3_L3_gr_info`: one granule of one channel.
#[derive(Clone, Copy, Debug)]
pub(super) struct GrInfo {
    /// Scale factor band widths, ending with 0.
    pub sfbtab: &'static [u8],
    pub part_23_length: u16,
    pub big_values: u16,
    pub scalefac_compress: u16,
    pub global_gain: u8,
    pub block_type: u8,
    pub mixed_block_flag: u8,
    pub n_long_sfb: u8,
    pub n_short_sfb: u8,
    pub table_select: [u8; 3],
    pub region_count: [u8; 3],
    pub subblock_gain: [u8; 3],
    pub preflag: u8,
    pub scalefac_scale: u8,
    pub count1_table: u8,
    pub scfsi: u8,
}

impl Default for GrInfo {
    fn default() -> Self {
        Self {
            sfbtab: &SCF_LONG[0],
            part_23_length: 0,
            big_values: 0,
            scalefac_compress: 0,
            global_gain: 0,
            block_type: 0,
            mixed_block_flag: 0,
            n_long_sfb: 0,
            n_short_sfb: 0,
            table_select: [0; 3],
            region_count: [0; 3],
            subblock_gain: [0; 3],
            preflag: 0,
            scalefac_scale: 0,
            count1_table: 0,
            scfsi: 0,
        }
    }
}

/// `drmp3_L3_read_side_info`: main_data_begin, or -1 for an invalid frame.
pub(super) fn read_side_info(bs: &mut Bs, buf: &[u8], gr: &mut [GrInfo; 4], hdr: &[u8; 4]) -> i32 {
    let mut scfsi = 0_u32;
    let mut part_23_sum = 0_i32;
    let mut gr_count = if is_mono(hdr) { 1 } else { 2 };
    let mut sr_idx = get_my_sample_rate(hdr) as usize;
    sr_idx = (sr_idx - usize::from(sr_idx != 0)).min(7);
    let mpeg1 = test_mpeg1(hdr);

    let main_data_begin = if mpeg1 {
        gr_count *= 2;
        let begin = bs.get_bits(buf, 9) as i32;
        scfsi = bs.get_bits(buf, 7 + gr_count);
        begin
    } else {
        (bs.get_bits(buf, 8 + gr_count) >> gr_count) as i32
    };

    for gr in gr.iter_mut().take(gr_count as usize) {
        if is_mono(hdr) {
            scfsi <<= 4;
        }
        gr.part_23_length = bs.get_bits(buf, 12) as u16;
        part_23_sum += i32::from(gr.part_23_length);
        gr.big_values = bs.get_bits(buf, 9) as u16;
        if gr.big_values > 288 {
            return -1;
        }
        gr.global_gain = bs.get_bits(buf, 8) as u8;
        gr.scalefac_compress = bs.get_bits(buf, if mpeg1 { 4 } else { 9 }) as u16;
        gr.sfbtab = &SCF_LONG[sr_idx];
        gr.n_long_sfb = 22;
        gr.n_short_sfb = 0;
        let tables;
        if bs.get_bits(buf, 1) != 0 {
            gr.block_type = bs.get_bits(buf, 2) as u8;
            if gr.block_type == 0 {
                return -1;
            }
            gr.mixed_block_flag = bs.get_bits(buf, 1) as u8;
            gr.region_count[0] = 7;
            gr.region_count[1] = 255;
            if gr.block_type == SHORT_BLOCK_TYPE {
                scfsi &= 0x0F0F;
                if gr.mixed_block_flag == 0 {
                    gr.region_count[0] = 8;
                    gr.sfbtab = SCF_SHORT[sr_idx];
                    gr.n_long_sfb = 0;
                    gr.n_short_sfb = 39;
                } else {
                    gr.sfbtab = SCF_MIXED[sr_idx];
                    gr.n_long_sfb = if mpeg1 { 8 } else { 6 };
                    gr.n_short_sfb = 30;
                }
            }
            tables = bs.get_bits(buf, 10) << 5;
            gr.subblock_gain[0] = bs.get_bits(buf, 3) as u8;
            gr.subblock_gain[1] = bs.get_bits(buf, 3) as u8;
            gr.subblock_gain[2] = bs.get_bits(buf, 3) as u8;
        } else {
            gr.block_type = 0;
            gr.mixed_block_flag = 0;
            tables = bs.get_bits(buf, 15);
            gr.region_count[0] = bs.get_bits(buf, 4) as u8;
            gr.region_count[1] = bs.get_bits(buf, 3) as u8;
            gr.region_count[2] = 255;
        }
        gr.table_select[0] = (tables >> 10) as u8;
        gr.table_select[1] = ((tables >> 5) & 31) as u8;
        gr.table_select[2] = (tables & 31) as u8;
        gr.preflag = if mpeg1 {
            bs.get_bits(buf, 1) as u8
        } else {
            u8::from(gr.scalefac_compress >= 500)
        };
        gr.scalefac_scale = bs.get_bits(buf, 1) as u8;
        gr.count1_table = bs.get_bits(buf, 1) as u8;
        gr.scfsi = ((scfsi >> 12) & 15) as u8;
        scfsi <<= 4;
    }

    if i64::from(part_23_sum) + i64::from(bs.pos)
        > i64::from(bs.limit) + i64::from(main_data_begin) * 8
    {
        return -1;
    }
    main_data_begin
}

/// `drmp3_L3_read_scalefactors`: reads the scale factors of up to four
/// partitions into `scf`, or copies them from the previous granule's
/// (`ist_pos`) where `scfsi` says so, and ends them with three zeros.
fn read_scalefactors(
    scf: &mut [u8; 40],
    ist_pos: &mut [u8; 39],
    scf_size: &[u8; 4],
    scf_count: &[u8],
    bs: &mut Bs,
    buf: &[u8],
    mut scfsi: i32,
) {
    let mut at = 0_usize;
    for (i, &count) in scf_count.iter().take(4).enumerate() {
        if count == 0 {
            break;
        }
        let cnt = usize::from(count);
        if scfsi & 8 != 0 {
            scf[at..at + cnt].copy_from_slice(&ist_pos[at..at + cnt]);
        } else {
            let bits = u32::from(scf_size[i]);
            if bits == 0 {
                scf[at..at + cnt].fill(0);
                ist_pos[at..at + cnt].fill(0);
            } else {
                let max_scf = if scfsi < 0 { (1 << bits) - 1 } else { -1 };
                for k in 0..cnt {
                    let s = bs.get_bits(buf, bits) as i32;
                    ist_pos[at + k] = if s == max_scf { 255 } else { s as u8 };
                    scf[at + k] = s as u8;
                }
            }
        }
        at += cnt;
        scfsi = scfsi.wrapping_mul(2);
    }
    scf[at..at + 3].fill(0);
}

/// `drmp3_L3_ldexp_q2`: `y` times 2 to the power of `-exp_q2 / 4`.
pub(super) fn ldexp_q2(mut y: f32, mut exp_q2: i32) -> f32 {
    const EXPFRAC: [f32; 4] = [
        9.31322575e-10,
        7.83145814e-10,
        6.58544508e-10,
        5.53767716e-10,
    ];
    loop {
        let e = exp_q2.min(30 * 4);
        y *= EXPFRAC[(e & 3) as usize] * (1_i32 << 30).wrapping_shr((e >> 2) as u32) as f32;
        exp_q2 -= e;
        if exp_q2 <= 0 {
            return y;
        }
    }
}

/// `drmp3_L3_decode_scalefactors`: the scale factors of granule `gr` of
/// channel `ch`, as gains in `scf`.
pub(super) fn decode_scalefactors(
    hdr: &[u8; 4],
    ist_pos: &mut [u8; 39],
    bs: &mut Bs,
    buf: &[u8],
    gr: &GrInfo,
    scf: &mut [f32],
    ch: usize,
) {
    const SCF_PARTITIONS: [[u8; 28]; 3] = [
        [
            6, 5, 5, 5, 6, 5, 5, 5, 6, 5, 7, 3, 11, 10, 0, 0, 7, 7, 7, 0, 6, 6, 6, 3, 8, 8, 5, 0,
        ],
        [
            8, 9, 6, 12, 6, 9, 9, 9, 6, 9, 12, 6, 15, 18, 0, 0, 6, 15, 12, 0, 6, 12, 9, 6, 6, 18,
            9, 0,
        ],
        [
            9, 9, 6, 12, 9, 9, 9, 9, 9, 9, 12, 6, 18, 18, 0, 0, 12, 12, 12, 0, 12, 9, 9, 6, 15, 12,
            9, 0,
        ],
    ];
    let row = usize::from(gr.n_short_sfb != 0) + usize::from(gr.n_long_sfb == 0);
    let scf_partition = &SCF_PARTITIONS[row];
    let mut scf_size = [0_u8; 4];
    let mut iscf = [0_u8; 40];
    let scf_shift = u32::from(gr.scalefac_scale) + 1;
    let mut scfsi = i32::from(gr.scfsi);
    let mut partition = 0;

    if test_mpeg1(hdr) {
        const SCFC_DECODE: [u8; 16] = [0, 1, 2, 3, 12, 5, 6, 7, 9, 10, 11, 13, 14, 15, 18, 19];
        let part = SCFC_DECODE[usize::from(gr.scalefac_compress & 15)];
        scf_size[0] = part >> 2;
        scf_size[1] = part >> 2;
        scf_size[2] = part & 3;
        scf_size[3] = part & 3;
    } else {
        const MOD: [i32; 6 * 4] = [
            5, 5, 4, 4, 5, 5, 4, 1, 4, 3, 1, 1, 5, 6, 6, 1, 4, 4, 4, 1, 4, 3, 1, 1,
        ];
        let ist = usize::from(test_i_stereo(hdr) && ch != 0);
        let mut sfc = i32::from(gr.scalefac_compress >> ist);
        let mut k = ist * 3 * 4;
        while sfc >= 0 {
            let mut modprod = 1;
            for i in (0..4).rev() {
                let m = MOD.get(k + i).copied().unwrap_or(1);
                scf_size[i] = (sfc / modprod % m) as u8;
                modprod *= m;
            }
            sfc -= modprod;
            k += 4;
        }
        partition = k.min(24);
        scfsi = -16;
    }
    read_scalefactors(
        &mut iscf,
        ist_pos,
        &scf_size,
        &scf_partition[partition..],
        bs,
        buf,
        scfsi,
    );

    let (n_long, n_short) = (usize::from(gr.n_long_sfb), usize::from(gr.n_short_sfb));
    if n_short != 0 {
        let sh = 3 - scf_shift;
        for i in (0..n_short).step_by(3) {
            for (j, &gain) in gr.subblock_gain.iter().enumerate() {
                let at = n_long + i + j;
                iscf[at] = iscf[at].wrapping_add(gain << sh);
            }
        }
    } else if gr.preflag != 0 {
        const PREAMP: [u8; 10] = [1, 1, 1, 1, 2, 2, 3, 3, 3, 2];
        for (i, &preamp) in PREAMP.iter().enumerate() {
            iscf[11 + i] = iscf[11 + i].wrapping_add(preamp);
        }
    }

    let ms = if is_ms_stereo(hdr) { 2 } else { 0 };
    let gain_exp = i32::from(gr.global_gain) + BITS_DEQUANTIZER_OUT * 4 - 210 - ms;
    let gain = ldexp_q2((1 << (MAX_SCFI / 4)) as f32, MAX_SCFI - gain_exp);
    for (out, &iscf) in scf.iter_mut().zip(&iscf).take(n_long + n_short) {
        *out = ldexp_q2(gain, i32::from(iscf) << scf_shift);
    }
}
