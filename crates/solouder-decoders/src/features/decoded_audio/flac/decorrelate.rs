// Ported from dr_flac 0.12.31 by David Reid (public domain or MIT No
// Attribution), as patched for SoLouder's C++ reference; altered for SoLouder.

//! Stereo decorrelation and left-justification of decoded subframes, in the
//! 32-bit wrapping arithmetic of dr_flac's scalar `read_pcm_frames_f32`
//! paths. Its SSE2 paths give the same results for valid files.

use super::frame::{LEFT_SIDE, MID_SIDE, RIGHT_SIDE};

/// Turns the subframes in `planes` (channel `c` at `c * stride`, `block`
/// samples each) into left-justified 32-bit samples. `unused` is 32 less
/// STREAMINFO's bits per sample; `wasted` holds each subframe's wasted bits.
pub(super) fn justify(
    assignment: u8,
    planes: &mut [i32],
    stride: usize,
    block: usize,
    wasted: &[u32],
    unused: u32,
) {
    if !matches!(assignment, LEFT_SIDE | RIGHT_SIDE | MID_SIDE) {
        for (c, &w) in wasted.iter().enumerate() {
            for sample in &mut planes[c * stride..][..block] {
                *sample = shl(*sample, unused + w) as i32;
            }
        }
        return;
    }
    let (first, rest) = planes.split_at_mut(stride);
    let pairs = first[..block].iter_mut().zip(&mut rest[..block]);
    match assignment {
        LEFT_SIDE => {
            for (left, side) in pairs {
                let l = shl(*left, unused + wasted[0]);
                let s = shl(*side, unused + wasted[1]);
                (*left, *side) = (l as i32, l.wrapping_sub(s) as i32);
            }
        }
        RIGHT_SIDE => {
            for (side, right) in pairs {
                let s = shl(*side, unused + wasted[0]);
                let r = shl(*right, unused + wasted[1]);
                (*side, *right) = (r.wrapping_add(s) as i32, r as i32);
            }
        }
        _ => {
            for (mid, side) in pairs {
                let s = shl(*side, wasted[1]);
                let m = (shl(*mid, wasted[0]) << 1) | (s & 1);
                let left = (m.wrapping_add(s) as i32) >> 1;
                let right = (m.wrapping_sub(s) as i32) >> 1;
                (*mid, *side) = (shl(left, unused) as i32, shl(right, unused) as i32);
            }
        }
    }
}

/// `sample << shift` in 32 bits, the shift taken modulo 32 as x86 does.
#[inline]
fn shl(sample: i32, shift: u32) -> u32 {
    (sample as u32).wrapping_shl(shift)
}
