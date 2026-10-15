// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! Layer 3 granules and the bit reservoir (minimp3's `drmp3_L3_decode`,
//! `drmp3_L3_save_reservoir` and `drmp3_L3_restore_reservoir`).

use super::bits::Bs;
use super::decoder::{MAX_BITRESERVOIR_BYTES, Scratch};
use super::header::{get_my_sample_rate, is_ms_stereo, test_i_stereo};
use super::huffman::huffman;
use super::imdct::{change_sign, imdct_gr};
use super::side_info::decode_scalefactors;
use super::stereo::{antialias, intensity_stereo, midside_stereo, reorder};

/// Where `drmp3dec_scratch.scf` starts in [`Scratch::grbuf`]: C's scratch
/// holds `grbuf[2][576]` and then `scf[40]`, and the reordering of
/// MPEG-2.5 8 kHz mixed blocks reads and writes past `grbuf[1]` into it.
pub(super) const SCF_AT: usize = 2 * 576;

/// `drmp3_L3_decode`: one granule of `nch` channels whose side information
/// starts at `gr_info[gr_at]`, from the main data to `s.grbuf`, through
/// the inverse MDCT with the overlaps in `mdct_overlap`.
pub(super) fn decode(
    header: &[u8; 4],
    mdct_overlap: &mut [[f32; 9 * 32]; 2],
    s: &mut Scratch,
    gr_at: usize,
    nch: usize,
) {
    for ch in 0..nch {
        let gr = s.gr_info[gr_at + ch];
        let layer3gr_limit = s.bs.pos.wrapping_add(i32::from(gr.part_23_length));
        let (grbuf, scf) = s.grbuf.split_at_mut(SCF_AT);
        decode_scalefactors(
            header,
            &mut s.ist_pos[ch],
            &mut s.bs,
            &s.maindata,
            &gr,
            scf,
            ch,
        );
        let dst = &mut grbuf[576 * ch..576 * ch + 576];
        huffman(dst, &mut s.bs, &s.maindata, &gr, scf, layer3gr_limit);
    }

    if test_i_stereo(header) {
        intensity_stereo(&mut s.grbuf, &mut s.ist_pos[1], &s.gr_info[gr_at..], header);
    } else if is_ms_stereo(header) {
        midside_stereo(&mut s.grbuf, 0, 576);
    }

    for (ch, overlap) in mdct_overlap.iter_mut().enumerate().take(nch) {
        let gr = s.gr_info[gr_at + ch];
        let mut aa_bands = 31;
        let mixed = if gr.mixed_block_flag != 0 { 2 } else { 0 };
        let n_long_bands = mixed << u32::from(get_my_sample_rate(header) == 2);

        if gr.n_short_sfb != 0 {
            aa_bands = n_long_bands as i32 - 1;
            let sfb = gr
                .sfbtab
                .get(usize::from(gr.n_long_sfb)..)
                .unwrap_or_default();
            reorder(&mut s.grbuf, 576 * ch + n_long_bands * 18, &mut s.syn, sfb);
        }

        antialias(&mut s.grbuf, 576 * ch, aa_bands);
        imdct_gr(&mut s.grbuf, 576 * ch, overlap, gr.block_type, n_long_bands);
        change_sign(&mut s.grbuf, 576 * ch);
    }
}

/// `drmp3_L3_save_reservoir`: keeps the main data after what the frame
/// read, at most 511 bytes, for the frames after it.
pub(super) fn save_reservoir(reserv: &mut i32, reserv_buf: &mut [u8; 511], s: &Scratch) {
    // C divides by 8u: in unsigned arithmetic, converted back to int.
    let mut pos = ((s.bs.pos as u32).wrapping_add(7) / 8) as i32;
    let mut remains = (s.bs.limit as u32 / 8).wrapping_sub(pos as u32) as i32;
    if remains > MAX_BITRESERVOIR_BYTES {
        pos += remains - MAX_BITRESERVOIR_BYTES;
        remains = MAX_BITRESERVOIR_BYTES;
    }
    if remains > 0 {
        let (pos, len) = (pos as usize, remains as usize);
        if let Some(bytes) = s.maindata.get(pos..pos + len) {
            reserv_buf[..len].copy_from_slice(bytes);
        }
    }
    *reserv = remains;
}

/// `drmp3_L3_restore_reservoir`: the main data of the frame, the bytes it
/// needs from the reservoir followed by the rest of the frame (`frame`
/// from `bs`), into `s.maindata`. False when the reservoir lacks bytes the
/// frame starts with.
pub(super) fn restore_reservoir(
    reserv: i32,
    reserv_buf: &[u8; 511],
    bs: &Bs,
    frame: &[u8],
    s: &mut Scratch,
    main_data_begin: i32,
) -> bool {
    let frame_bytes = ((bs.limit - bs.pos) / 8).max(0) as usize;
    let bytes_have = reserv.min(main_data_begin).max(0) as usize;
    let from = (reserv - main_data_begin).max(0) as usize;
    if let Some(bytes) = reserv_buf.get(from..from + bytes_have) {
        s.maindata[..bytes_have].copy_from_slice(bytes);
    }
    let start = (bs.pos / 8).max(0) as usize;
    let len = frame_bytes
        .min(frame.len().saturating_sub(start))
        .min(s.maindata.len() - bytes_have);
    s.maindata[bytes_have..bytes_have + len].copy_from_slice(&frame[start..start + len]);
    s.bs = Bs::new((bytes_have + frame_bytes) as i32);
    reserv >= main_data_begin
}
