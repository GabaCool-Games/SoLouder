// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! MPEG audio frame headers and the frame search (minimp3's `drmp3_hdr_*`,
//! `drmp3d_match_frame` and `drmp3d_find_frame`).

/// `DRMP3_HDR_SIZE`.
pub(super) const HDR_SIZE: i32 = 4;
/// `DRMP3_MAX_FREE_FORMAT_FRAME_SIZE`.
pub(super) const MAX_FREE_FORMAT_FRAME_SIZE: i32 = 2304;
/// `DRMP3_MAX_FRAME_SYNC_MATCHES`.
const MAX_FRAME_SYNC_MATCHES: i32 = 10;

/// The four header bytes at `at`, zero past the end of `buf` (minimp3
/// checks the length before every read that matters).
pub(super) fn hdr_at(buf: &[u8], at: i32) -> [u8; 4] {
    let mut h = [0; 4];
    if let Ok(at) = usize::try_from(at) {
        for (i, b) in h.iter_mut().enumerate() {
            *b = buf.get(at + i).copied().unwrap_or(0);
        }
    }
    h
}

pub(super) fn is_mono(h: &[u8; 4]) -> bool {
    h[3] & 0xC0 == 0xC0
}
pub(super) fn is_ms_stereo(h: &[u8; 4]) -> bool {
    h[3] & 0xE0 == 0x60
}
pub(super) fn is_free_format(h: &[u8; 4]) -> bool {
    h[2] & 0xF0 == 0
}
pub(super) fn is_crc(h: &[u8; 4]) -> bool {
    h[1] & 1 == 0
}
fn test_padding(h: &[u8; 4]) -> bool {
    h[2] & 0x2 != 0
}
pub(super) fn test_mpeg1(h: &[u8; 4]) -> bool {
    h[1] & 0x8 != 0
}
fn test_not_mpeg25(h: &[u8; 4]) -> bool {
    h[1] & 0x10 != 0
}
pub(super) fn test_i_stereo(h: &[u8; 4]) -> bool {
    h[3] & 0x10 != 0
}
pub(super) fn test_ms_stereo(h: &[u8; 4]) -> bool {
    h[3] & 0x20 != 0
}
pub(super) fn get_stereo_mode(h: &[u8; 4]) -> u32 {
    u32::from((h[3] >> 6) & 3)
}
pub(super) fn get_stereo_mode_ext(h: &[u8; 4]) -> u32 {
    u32::from((h[3] >> 4) & 3)
}
pub(super) fn get_layer(h: &[u8; 4]) -> u32 {
    u32::from((h[1] >> 1) & 3)
}
fn get_bitrate(h: &[u8; 4]) -> u32 {
    u32::from(h[2] >> 4)
}
pub(super) fn get_sample_rate(h: &[u8; 4]) -> u32 {
    u32::from((h[2] >> 2) & 3)
}
/// `DRMP3_HDR_GET_MY_SAMPLE_RATE`: 0 to 8, MPEG-2.5 first.
pub(super) fn get_my_sample_rate(h: &[u8; 4]) -> u32 {
    get_sample_rate(h) + (u32::from((h[1] >> 3) & 1) + u32::from((h[1] >> 4) & 1)) * 3
}
fn is_frame_576(h: &[u8; 4]) -> bool {
    h[1] & 14 == 2
}
pub(super) fn is_layer_1(h: &[u8; 4]) -> bool {
    h[1] & 6 == 6
}

/// `drmp3_hdr_valid`.
pub(super) fn hdr_valid(h: &[u8; 4]) -> bool {
    h[0] == 0xff
        && (h[1] & 0xF0 == 0xf0 || h[1] & 0xFE == 0xe2)
        && get_layer(h) != 0
        && get_bitrate(h) != 15
        && get_sample_rate(h) != 3
}

/// `drmp3_hdr_compare`: whether `h2` is a valid header of the same stream.
pub(super) fn hdr_compare(h1: &[u8; 4], h2: &[u8; 4]) -> bool {
    hdr_valid(h2)
        && (h1[1] ^ h2[1]) & 0xFE == 0
        && (h1[2] ^ h2[2]) & 0x0C == 0
        && is_free_format(h1) == is_free_format(h2)
}

/// `drmp3_hdr_bitrate_kbps`; 0 for free format.
pub(super) fn bitrate_kbps(h: &[u8; 4]) -> u32 {
    const HALFRATE: [[[u8; 15]; 3]; 2] = [
        [
            [0, 4, 8, 12, 16, 20, 24, 28, 32, 40, 48, 56, 64, 72, 80],
            [0, 4, 8, 12, 16, 20, 24, 28, 32, 40, 48, 56, 64, 72, 80],
            [0, 16, 24, 28, 32, 40, 48, 56, 64, 72, 80, 88, 96, 112, 128],
        ],
        [
            [0, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160],
            [
                0, 16, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192,
            ],
            [
                0, 16, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224,
            ],
        ],
    ];
    let rates = &HALFRATE[usize::from(test_mpeg1(h))];
    let rate = (get_layer(h) as usize)
        .checked_sub(1)
        .and_then(|layer| rates[layer].get(get_bitrate(h) as usize));
    2 * u32::from(rate.copied().unwrap_or(0))
}

/// `drmp3_hdr_sample_rate_hz`.
pub(super) fn sample_rate_hz(h: &[u8; 4]) -> u32 {
    const HZ: [u32; 3] = [44100, 48000, 32000];
    let hz = HZ.get(get_sample_rate(h) as usize).copied().unwrap_or(0);
    hz >> u32::from(!test_mpeg1(h)) >> u32::from(!test_not_mpeg25(h))
}

/// `drmp3_hdr_frame_samples`: 384, 576 or 1152.
pub(super) fn frame_samples(h: &[u8; 4]) -> u32 {
    if is_layer_1(h) {
        384
    } else {
        1152 >> u32::from(is_frame_576(h))
    }
}

/// `drmp3_hdr_frame_bytes`: the frame length without padding, or
/// `free_format_size` for a free-format frame.
pub(super) fn frame_bytes(h: &[u8; 4], free_format_size: i32) -> i32 {
    let hz = sample_rate_hz(h);
    // hz is never 0 for a valid header; 0 bytes then means free format.
    let mut bytes = (frame_samples(h) * bitrate_kbps(h) * 125)
        .checked_div(hz)
        .unwrap_or(0) as i32;
    if is_layer_1(h) {
        bytes &= !3; // slot align
    }
    if bytes != 0 { bytes } else { free_format_size }
}

/// `drmp3_hdr_padding`.
pub(super) fn padding(h: &[u8; 4]) -> i32 {
    if test_padding(h) {
        if is_layer_1(h) { 4 } else { 1 }
    } else {
        0
    }
}

/// `drmp3d_match_frame`: whether the frames after the one at the start of
/// `mp3` continue the stream, as far as `mp3_bytes` shows them.
fn match_frame(mp3: &[u8], mp3_bytes: i32, frame_bytes: i32) -> bool {
    let first = hdr_at(mp3, 0);
    let mut i = 0_i32;
    for nmatch in 0..MAX_FRAME_SYNC_MATCHES {
        let h = hdr_at(mp3, i);
        i = i
            .wrapping_add(self::frame_bytes(&h, frame_bytes))
            .wrapping_add(padding(&h));
        if i.wrapping_add(HDR_SIZE) > mp3_bytes {
            return nmatch > 0;
        }
        if !hdr_compare(&first, &hdr_at(mp3, i)) {
            return false;
        }
    }
    true
}

/// `drmp3d_find_frame`: the offset of the first frame in `mp3` that the
/// frames after it confirm, and its length with padding (0 if none was
/// found, in which case the offset is `mp3_bytes`). A free-format frame's
/// length is measured and kept in `free_format_bytes`.
pub(super) fn find_frame(mp3: &[u8], mp3_bytes: i32, free_format_bytes: &mut i32) -> (i32, i32) {
    // Sums in 64 bits: `mp3_bytes` may be close to `i32::MAX`.
    let n = i64::from(mp3_bytes);
    let size = i64::from(HDR_SIZE);
    let mut i = 0_i32;
    while i64::from(i) < n - size {
        let at = mp3.get(i as usize..).unwrap_or_default();
        let h = hdr_at(at, 0);
        if hdr_valid(&h) {
            let mut frame_bytes = frame_bytes(&h, *free_format_bytes);
            let mut frame_and_padding = frame_bytes + padding(&h);

            let mut k = HDR_SIZE;
            while frame_bytes == 0
                && k < MAX_FREE_FORMAT_FRAME_SIZE
                && i64::from(i) + 2 * i64::from(k) < n - size
            {
                if hdr_compare(&h, &hdr_at(at, k)) {
                    let fb = k - padding(&h);
                    let nextfb = fb + padding(&hdr_at(at, k));
                    if i64::from(i) + i64::from(k + nextfb) + size > n
                        || !hdr_compare(&h, &hdr_at(at, k + nextfb))
                    {
                        k += 1;
                        continue;
                    }
                    frame_and_padding = k;
                    frame_bytes = fb;
                    *free_format_bytes = fb;
                }
                k += 1;
            }

            if (frame_bytes != 0
                && i64::from(i) + i64::from(frame_and_padding) <= n
                && match_frame(at, mp3_bytes - i, frame_bytes))
                || (i == 0 && frame_and_padding == mp3_bytes)
            {
                return (i, frame_and_padding);
            }
            *free_format_bytes = 0;
        }
        i += 1;
    }
    (mp3_bytes, 0)
}
