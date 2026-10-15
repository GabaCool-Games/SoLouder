// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! Reading codewords and their vectors: `codebook_decode_scalar_raw`, the
//! `DECODE_RAW` and `DECODE` macros, `codebook_decode`,
//! `codebook_decode_step` and `codebook_decode_deinterleave_repeat`.

use super::codebook::{Codebook, FAST_HUFFMAN_LENGTH, FAST_HUFFMAN_TABLE_MASK, NO_CODE};
use super::ogg::{Reader, VorbisError};
use super::source::Source;
use super::tables::bit_reverse;

/// `codebook_decode_scalar_raw`: the codeword after the fast table missed,
/// by binary or linear search; -1 if none.
fn codebook_decode_scalar_raw<S: Source>(f: &mut Reader<S>, c: &Codebook) -> i32 {
    f.prep_huffman();
    if c.codewords.is_empty() && c.sorted_codewords.is_empty() {
        return -1;
    }
    // cases to use binary search: sorted_codewords && !c->codewords
    //                             sorted_codewords && c->entries > 8
    let binary = if c.entries > 8 {
        !c.sorted_codewords.is_empty()
    } else {
        c.codewords.is_empty()
    };
    if binary {
        let mut x = c.search_sorted(bit_reverse(f.acc));
        // x is now the sorted index
        if !c.sparse {
            x = c.sorted_value(x);
        }
        // x is now sorted index if sparse, or symbol otherwise
        let len = i32::from(
            c.codeword_lengths
                .get(x as usize)
                .copied()
                .unwrap_or(NO_CODE),
        );
        if f.valid_bits >= len {
            f.acc = f.acc.wrapping_shr(len as u32);
            f.valid_bits -= len;
            return x;
        }
        f.valid_bits = 0;
        return -1;
    }
    // if small, linear search (never sparse here)
    for i in 0..c.entries as usize {
        let len = c.codeword_lengths[i];
        if len == NO_CODE {
            continue;
        }
        let mask = 1_u32.wrapping_shl(u32::from(len)).wrapping_sub(1);
        if c.codewords[i] == f.acc & mask {
            if f.valid_bits >= i32::from(len) {
                f.acc = f.acc.wrapping_shr(u32::from(len));
                f.valid_bits -= i32::from(len);
                return i as i32;
            }
            f.valid_bits = 0;
            return -1;
        }
    }
    f.error(VorbisError::InvalidStream);
    f.valid_bits = 0;
    -1
}

/// `DECODE_RAW` (also `DECODE_VQ`, stb's default): the codeword's index,
/// sorted when the codebook is sparse; -1 if none.
#[inline]
pub(super) fn decode_raw<S: Source>(f: &mut Reader<S>, c: &Codebook) -> i32 {
    if f.valid_bits < i32::from(FAST_HUFFMAN_LENGTH) {
        f.prep_huffman();
    }
    let var = i32::from(c.fast_huffman[(f.acc & FAST_HUFFMAN_TABLE_MASK) as usize]);
    if var >= 0 {
        let n = i32::from(c.codeword_lengths[var as usize]);
        f.acc >>= n;
        f.valid_bits -= n;
        if f.valid_bits < 0 {
            f.valid_bits = 0;
            return -1;
        }
        var
    } else {
        codebook_decode_scalar_raw(f, c)
    }
}

/// `DECODE`: the codeword's symbol; -1 if none.
#[inline]
pub(super) fn decode<S: Source>(f: &mut Reader<S>, c: &Codebook) -> i32 {
    let var = decode_raw(f, c);
    if c.sparse { c.sorted_value(var) } else { var }
}

/// `codebook_decode_start`.
fn codebook_decode_start<S: Source>(f: &mut Reader<S>, c: &Codebook) -> i32 {
    let mut z = -1;
    // type 0 is only legal in a scalar context
    if c.lookup_type == 0 {
        f.error(VorbisError::InvalidStream);
    } else {
        z = decode_raw(f, c);
        if z < 0 {
            // check for EOP
            if f.bytes_in_seg == 0 && f.last_seg {
                return z;
            }
            f.error(VorbisError::InvalidStream);
        }
    }
    z
}

/// The values of vector `z`, at most `len` of them; `None` where the C
/// would read outside the table.
fn vector(c: &Codebook, z: i32, len: i32) -> Option<&[f32]> {
    let len = len.min(c.dimensions).max(0) as usize;
    let at = z as usize * c.dimensions as usize;
    c.multiplicands.get(at..at + len)
}

/// `codebook_decode`: adds the next vector to `output` (at most `len`
/// values).
pub(super) fn codebook_decode<S: Source>(
    f: &mut Reader<S>,
    c: &Codebook,
    output: &mut [f32],
    len: i32,
) -> bool {
    let z = codebook_decode_start(f, c);
    if z < 0 {
        return false;
    }
    let Some(values) = vector(c, z, len) else {
        return false;
    };
    let Some(output) = output.get_mut(..values.len()) else {
        return false;
    };
    let mut last = 0.0_f32;
    if c.sequence_p {
        for (out, &value) in output.iter_mut().zip(values) {
            let val = value + last;
            *out += val;
            last = val + c.minimum_value;
        }
    } else {
        for (out, &value) in output.iter_mut().zip(values) {
            *out += value + last;
        }
    }
    true
}

/// `codebook_decode_step`: adds the next vector to every `step`th value of
/// `output`.
pub(super) fn codebook_decode_step<S: Source>(
    f: &mut Reader<S>,
    c: &Codebook,
    output: &mut [f32],
    len: i32,
    step: i32,
) -> bool {
    let z = codebook_decode_start(f, c);
    let mut last = 0.0_f32;
    if z < 0 {
        return false;
    }
    let Some(values) = vector(c, z, len) else {
        return false;
    };
    for (i, &value) in values.iter().enumerate() {
        let val = value + last;
        let Some(out) = output.get_mut(i * step as usize) else {
            return false;
        };
        *out += val;
        if c.sequence_p {
            last = val;
        }
    }
    true
}

/// Where `codebook_decode_deinterleave_repeat` writes: the channel buffers,
/// `stride` apart, and the channel of each interleaved slot (`None` for
/// stb's null pointer).
pub(super) struct Interleaved<'a> {
    pub buffers: &'a mut [f32],
    pub stride: usize,
    pub channels: &'a [Option<usize>],
}

/// `codebook_decode_deinterleave_repeat`: decodes vectors into the
/// interleaved channels from (`c_inter`, `p_inter`) until `total_decode`
/// values are read, `len` values per channel.
pub(super) fn codebook_decode_deinterleave_repeat<S: Source>(
    f: &mut Reader<S>,
    c: &Codebook,
    out: &mut Interleaved,
    inter: &mut (i32, i32),
    len: i32,
    mut total_decode: i32,
) -> bool {
    let ch = out.channels.len() as i32;
    let (mut c_inter, mut p_inter) = *inter;
    let mut effective = c.dimensions;
    // type 0 is only legal in a scalar context
    if c.lookup_type == 0 {
        return f.error(VorbisError::InvalidStream);
    }
    while total_decode > 0 {
        let mut last = 0.0_f32;
        let z = decode_raw(f, c);
        if z < 0 {
            if f.bytes_in_seg == 0 && f.last_seg {
                return false;
            }
            return f.error(VorbisError::InvalidStream);
        }
        // if this will take us off the end of the buffers, stop short!
        // (stb's correction overshoots by 2 * c_inter, so the last vector
        // may read past its own values and write past `len`, into the half
        // of the buffer the inverse MDCT does not read)
        if c_inter + p_inter * ch + effective > len * ch {
            effective = len * ch - (p_inter * ch - c_inter);
        }
        let base = z as usize * c.dimensions as usize;
        for i in 0..effective.max(0) as usize {
            // Values past the table (undefined in C) land past `len`.
            let val = c.multiplicands.get(base + i).copied().unwrap_or(0.0) + last;
            if let Some(Some(channel)) = out.channels.get(c_inter as usize) {
                let at = channel * out.stride + p_inter as usize;
                if p_inter as usize >= out.stride {
                    return false;
                }
                out.buffers[at] += val;
            }
            c_inter += 1;
            if c_inter == ch {
                c_inter = 0;
                p_inter += 1;
            }
            if c.sequence_p {
                last = val;
            }
        }
        total_decode -= effective;
    }
    *inter = (c_inter, p_inter);
    true
}
