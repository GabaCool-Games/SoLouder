// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! Floor type 1: decoding its points (the floor part of
//! `vorbis_decode_packet_rest`) and rendering it onto the residue
//! (`do_floor`, `draw_line`, `predict_point`), deferred as stb's default.

use super::codebook::Codebook;
use super::huffman::decode;
use super::ogg::{INVALID_BITS, Reader};
use super::setup::Floor1;
use super::source::Source;
use super::tables::{INVERSE_DB_TABLE, ilog};

fn predict_point(x: i32, x0: i32, x1: i32, y0: i32, y1: i32) -> i32 {
    let dy = y1.wrapping_sub(y0);
    let adx = x1 - x0;
    let err = dy.wrapping_abs().wrapping_mul(x - x0);
    // the points of a floor are distinct, so adx is never 0
    let off = err.checked_div(adx).unwrap_or(0);
    if dy < 0 {
        y0.wrapping_sub(off)
    } else {
        y0.wrapping_add(off)
    }
}

/// Decodes the floor of one channel into `final_y`, points left out set to
/// -1. False if the channel is silent (`zero_channel`), also when the
/// packet ended while reading it.
pub(super) fn decode_floor1<S: Source>(
    f: &mut Reader<S>,
    g: &Floor1,
    codebooks: &[Codebook],
    final_y: &mut [i16],
) -> bool {
    const RANGE_LIST: [i32; 4] = [256, 128, 86, 64];
    if f.get_bits(1) == 0 {
        return false;
    }
    let mut step2_flag = [false; 256];
    let range = RANGE_LIST[usize::from(g.floor1_multiplier - 1)];
    let mut offset = 2;
    final_y[0] = f.get_bits(ilog(range) - 1) as i16;
    final_y[1] = f.get_bits(ilog(range) - 1) as i16;
    for &pclass in &g.partition_class_list[..usize::from(g.partitions)] {
        let pclass = usize::from(pclass);
        let cdim = g.class_dimensions[pclass];
        let cbits = i32::from(g.class_subclasses[pclass]);
        let csub = (1 << cbits) - 1;
        let mut cval = 0;
        if cbits != 0 {
            let c = &codebooks[usize::from(g.class_masterbooks[pclass])];
            cval = decode(f, c);
        }
        for _ in 0..cdim {
            let book = g.subclass_books[pclass][(cval & csub) as usize];
            cval >>= cbits;
            final_y[offset] = if book >= 0 {
                decode(f, &codebooks[book as usize]) as i16
            } else {
                0
            };
            offset += 1;
        }
    }
    // behavior according to spec
    if f.valid_bits == INVALID_BITS {
        return false;
    }
    step2_flag[0] = true;
    step2_flag[1] = true;
    let values = g.values as usize;
    for j in 2..values {
        let [low, high] = g.neighbors[j].map(usize::from);
        let pred = predict_point(
            i32::from(g.xlist[j]),
            i32::from(g.xlist[low]),
            i32::from(g.xlist[high]),
            i32::from(final_y[low]),
            i32::from(final_y[high]),
        );
        let val = i32::from(final_y[j]);
        let highroom = range.wrapping_sub(pred);
        let lowroom = pred;
        let room = if highroom < lowroom {
            highroom.wrapping_mul(2)
        } else {
            lowroom.wrapping_mul(2)
        };
        if val != 0 {
            step2_flag[low] = true;
            step2_flag[high] = true;
            step2_flag[j] = true;
            final_y[j] = if val >= room {
                if highroom > lowroom {
                    val.wrapping_sub(lowroom).wrapping_add(pred)
                } else {
                    pred.wrapping_sub(val)
                        .wrapping_add(highroom)
                        .wrapping_sub(1)
                }
            } else if val & 1 != 0 {
                pred.wrapping_sub((val + 1) >> 1)
            } else {
                pred.wrapping_add(val >> 1)
            } as i16;
        } else {
            step2_flag[j] = false;
            final_y[j] = pred as i16;
        }
    }
    // defer final floor computation until _after_ residue
    for (y, &flag) in final_y.iter_mut().zip(&step2_flag).take(values) {
        if !flag {
            *y = -1;
        }
    }
    true
}

/// `draw_line` with `LINE_OP` multiplying (deferred floors).
#[inline]
fn draw_line(output: &mut [f32], x0: i32, y0: i32, mut x1: i32, y1: i32, n: i32) {
    let dy = y1.wrapping_sub(y0);
    let adx = x1 - x0;
    if adx <= 0 {
        // points are drawn left to right, so this never happens
        return;
    }
    let mut ady = dy.wrapping_abs();
    let base = dy / adx;
    let sy = if dy < 0 { base - 1 } else { base + 1 };
    let (mut x, mut y, mut err) = (x0, y0, 0_i32);
    ady = ady.wrapping_sub(base.wrapping_abs().wrapping_mul(adx));
    if x1 > n {
        x1 = n;
    }
    if x < x1 {
        output[x as usize] *= INVERSE_DB_TABLE[(y & 255) as usize];
        x += 1;
        while x < x1 {
            err = err.wrapping_add(ady);
            if err >= adx {
                err -= adx;
                y = y.wrapping_add(sy);
            } else {
                y = y.wrapping_add(base);
            }
            output[x as usize] *= INVERSE_DB_TABLE[(y & 255) as usize];
            x += 1;
        }
    }
}

/// `do_floor`: multiplies the first `n / 2` values of `target` by the
/// floor. False where the C would read outside `inverse_db_table` (its
/// last segment does not mask the level), which only damaged packets do.
pub(super) fn do_floor(g: &Floor1, n: i32, target: &mut [f32], final_y: &[i16]) -> bool {
    let n2 = n >> 1;
    let mult = i32::from(g.floor1_multiplier);
    let mut lx = 0;
    let mut ly = i32::from(final_y[0]) * mult;
    for &j in &g.sorted_order[1..g.values as usize] {
        let j = usize::from(j);
        if final_y[j] >= 0 {
            let hy = i32::from(final_y[j]) * mult;
            let hx = i32::from(g.xlist[j]);
            if lx != hx {
                draw_line(target, lx, ly, hx, hy, n2);
            }
            lx = hx;
            ly = hy;
        }
    }
    if lx < n2 {
        // optimization of: draw_line(target, lx,ly, n,ly, n2);
        let Some(&level) = usize::try_from(ly)
            .ok()
            .and_then(|ly| INVERSE_DB_TABLE.get(ly))
        else {
            return false;
        };
        for t in &mut target[lx as usize..n2 as usize] {
            *t *= level;
        }
    }
    true
}
