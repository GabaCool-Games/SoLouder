// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! Residue decoding: `decode_residue` and `residue_decode`, for types 0, 1
//! and 2, as stb does without `STB_VORBIS_DIVIDES_IN_RESIDUE`.

use super::codebook::Codebook;
use super::huffman::{
    Interleaved, codebook_decode, codebook_decode_deinterleave_repeat, codebook_decode_step, decode,
};
use super::ogg::{EOP, Reader};
use super::setup::Residue;
use super::source::Source;

/// `residue_decode`: one partition of `n` values at `offset` in `target`.
fn residue_decode<S: Source>(
    f: &mut Reader<S>,
    book: &Codebook,
    target: &mut [f32],
    offset: i32,
    n: i32,
    rtype: u16,
) -> bool {
    if rtype == 0 {
        let step = n / book.dimensions;
        for k in 0..step {
            let Some(out) = target.get_mut((offset + k) as usize..) else {
                return false;
            };
            if !codebook_decode_step(f, book, out, n - offset - k, step) {
                return false;
            }
        }
    } else {
        let (mut k, mut offset) = (0, offset);
        while k < n {
            let Some(out) = target.get_mut(offset as usize..) else {
                return false;
            };
            if !codebook_decode(f, book, out, n - k) {
                return false;
            }
            k += book.dimensions;
            offset += book.dimensions;
        }
    }
    true
}

/// The channel buffers being decoded into.
pub(super) struct Buffers<'a> {
    /// All channel buffers, `stride` apart.
    pub data: &'a mut [f32],
    pub stride: usize,
    /// `residue_buffers`: the channel of each of the `ch` channels being
    /// decoded, `None` where stb passes null (`do_not_decode`).
    pub channels: &'a [Option<usize>],
}

/// `decode_residue`: residue `r` for the channels in `out`, `n` (half the
/// blocksize) values each. `part_classdata` is scratch for stb's
/// `temp_block_array`. Stops where stb would `goto done`.
pub(super) fn decode_residue<S: Source>(
    f: &mut Reader<S>,
    codebooks: &[Codebook],
    r: &Residue,
    out: &mut Buffers,
    n: i32,
    part_classdata: &mut Vec<usize>,
) {
    let ch = out.channels.len();
    let rtype = r.rtype;
    let classbook = &codebooks[usize::from(r.classbook)];
    let classwords = classbook.dimensions as usize;
    let actual_size = if rtype == 2 { n as u32 * 2 } else { n as u32 };
    let limit_r_begin = r.begin.min(actual_size);
    let limit_r_end = r.end.min(actual_size);
    let n_read = limit_r_end.wrapping_sub(limit_r_begin) as i32;
    let part_read = (n_read as u32 / r.part_size) as usize;
    part_classdata.clear();
    part_classdata.resize(ch * part_read, 0);
    let part_size = r.part_size as i32;
    let begin = r.begin as i32;

    for &channel in out.channels {
        if let Some(c) = channel {
            out.data[c * out.stride..][..n as usize].fill(0.0);
        }
    }

    if rtype == 2 && ch != 1 {
        if out.channels.iter().all(Option::is_none) {
            return;
        }
        let mut target = Interleaved {
            buffers: &mut *out.data,
            stride: out.stride,
            channels: out.channels,
        };
        let chi = ch as i32;
        for pass in 0..8 {
            let (mut pcount, mut class_set) = (0, 0);
            while pcount < part_read {
                let z = begin.wrapping_add((pcount as i32).wrapping_mul(part_size));
                let mut inter = (z % chi, z / chi);
                if pass == 0 {
                    let q = decode(f, classbook);
                    if q == EOP {
                        return;
                    }
                    part_classdata[class_set] = q as usize * classwords;
                }
                let mut i = 0;
                while i < classwords && pcount < part_read {
                    let mut z = begin.wrapping_add((pcount as i32).wrapping_mul(part_size));
                    let c = r.classdata[part_classdata[class_set] + i];
                    let b = r.residue_books[usize::from(c)][pass];
                    if b >= 0 {
                        let book = &codebooks[b as usize];
                        if !codebook_decode_deinterleave_repeat(
                            f,
                            book,
                            &mut target,
                            &mut inter,
                            n,
                            part_size,
                        ) {
                            return;
                        }
                    } else {
                        z = z.wrapping_add(part_size);
                        inter = (z % chi, z / chi);
                    }
                    i += 1;
                    pcount += 1;
                }
                class_set += 1;
            }
        }
        return;
    }

    for pass in 0..8 {
        let (mut pcount, mut class_set) = (0, 0);
        while pcount < part_read {
            if pass == 0 {
                for (j, channel) in out.channels.iter().enumerate() {
                    if channel.is_some() {
                        let temp = decode(f, classbook);
                        if temp == EOP {
                            return;
                        }
                        part_classdata[j * part_read + class_set] = temp as usize * classwords;
                    }
                }
            }
            let mut i = 0;
            while i < classwords && pcount < part_read {
                for (j, channel) in out.channels.iter().enumerate() {
                    let Some(channel) = *channel else {
                        continue;
                    };
                    let c = r.classdata[part_classdata[j * part_read + class_set] + i];
                    let b = r.residue_books[usize::from(c)][pass];
                    if b >= 0 {
                        let target = &mut out.data[channel * out.stride..][..out.stride];
                        let offset = begin.wrapping_add((pcount as i32).wrapping_mul(part_size));
                        let book = &codebooks[b as usize];
                        if !residue_decode(f, book, target, offset, part_size, rtype) {
                            return;
                        }
                    }
                }
                i += 1;
                pcount += 1;
            }
            class_set += 1;
        }
    }
}
