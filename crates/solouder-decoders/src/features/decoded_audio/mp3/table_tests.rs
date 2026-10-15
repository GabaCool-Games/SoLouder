//! Frame headers and the decoder's tables: lengths, the frame search, and
//! that no table sends an index out of its array.

use super::super::header::{
    find_frame, frame_bytes, frame_samples, hdr_valid, padding, sample_rate_hz,
};
use super::super::huffman_tables::{LINBITS, TAB32, TAB33, TABINDEX, TABS};
use super::super::layer3::SCF_AT;
use super::super::tables::{SCF_LONG, SCF_MIXED, SCF_SHORT};

#[test]
fn headers_give_frame_lengths_and_the_search_skips_junk() {
    // MPEG-1 layer 3, 128 kbps, 44.1 kHz, padded: 417 bytes and 1.
    let h = [0xFF, 0xFB, 0x92, 0x64];
    assert!(hdr_valid(&h));
    assert_eq!((frame_bytes(&h, 0), padding(&h)), (417, 1));
    assert_eq!((frame_samples(&h), sample_rate_hz(&h)), (1152, 44100));
    // MPEG-2.5 layer 3 at 8 kHz has 576 frames; layer 1 has 384.
    assert_eq!(frame_samples(&[0xFF, 0xE3, 0x88, 0xC4]), 576);
    assert_eq!(sample_rate_hz(&[0xFF, 0xE3, 0x88, 0xC4]), 8000);
    assert_eq!(frame_samples(&[0xFF, 0xFF, 0xC4, 0x00]), 384);
    assert!(!hdr_valid(&[0xFF, 0xF9, 0xF0, 0x00]), "layer 0, bitrate 15");

    let file = fixture!("mpeg1_stereo_44100_cbr128.mp3");
    let mut free = 0;
    let (at, len) = find_frame(file, file.len() as i32, &mut free);
    let mut junked = vec![0xFF; 3];
    junked.extend_from_slice(&[0xFF, 0xFB, 0x00, 0x00, 0x12]);
    junked.extend_from_slice(file);
    let (at2, len2) = find_frame(&junked, junked.len() as i32, &mut free);
    assert_eq!((at2, len2), (at + 8, len));
}

#[test]
fn every_huffman_code_ends_inside_its_table() {
    // Walks every path of every big-value tree: subtree widths are 1 to 7
    // bits (C would shift by 32 for 0) and every index is in TABS.
    fn walk(base: i64, leaf: i32, depth: u32) {
        assert!(depth < 8, "tree too deep");
        let w = (leaf & 7) as u32;
        assert!(w >= 1);
        for v in 0..1_i64 << w {
            let at = base + v - i64::from(leaf >> 3);
            let next = i32::from(TABS[usize::try_from(at).unwrap()]);
            if next < 0 {
                walk(base, next, depth + 1);
            }
        }
    }
    for (table, &start) in TABINDEX.iter().enumerate() {
        let base = i64::from(start);
        for v in 0..32 {
            let leaf = i32::from(TABS[(base + v) as usize]);
            if leaf < 0 {
                walk(base, leaf, 1);
            }
        }
        assert!(LINBITS[table] <= 13);
    }
    // count1 codes: four bits, then 1 to 3 more within the table.
    for table in [&TAB32[..], &TAB33[..]] {
        for &leaf in &table[..16] {
            let leaf = usize::from(leaf);
            if leaf & 8 == 0 {
                let bits = leaf & 3;
                assert!(bits >= 1 && (leaf >> 3) + (1 << bits) <= table.len());
            }
        }
    }
}

#[test]
fn band_tables_cover_576_lines_and_reordering_stays_in_the_scratch() {
    for i in 0..8 {
        for table in [&SCF_LONG[i][..], SCF_SHORT[i], SCF_MIXED[i]] {
            assert_eq!(table.iter().map(|&w| usize::from(w)).sum::<usize>(), 576);
            assert_eq!(table.last(), Some(&0));
        }
        // Mixed blocks reorder after 2 long bands (4 at 8 kHz); the short
        // bands after the first 6 or 8 widths may pass channel 1's 576
        // lines, as in C, but stay within the scale factors after them.
        let (long_sfb, long_bands) = match i {
            1 => (6, 4),
            5..=7 => (8, 2),
            _ => (6, 2),
        };
        {
            let short: usize = SCF_MIXED[i][long_sfb..]
                .iter()
                .map(|&w| usize::from(w))
                .sum();
            assert!(576 + long_bands * 18 + short <= SCF_AT + 40);
        }
    }
}
