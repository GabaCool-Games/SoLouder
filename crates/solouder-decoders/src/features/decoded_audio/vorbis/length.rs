// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! Finding pages and the stream's length: `vorbis_find_page` and
//! `stb_vorbis_stream_length_in_samples`.

use super::decoder::Vorbis;
use super::ogg::ProbedPage;
use super::source::Source;
use super::tables::crc32_update;

pub(super) const OGG_PAGE_HEADER: [u8; 4] = *b"OggS";
const SAMPLE_UNKNOWN: u32 = 0xffff_ffff;

impl<S: Source> Vorbis<S> {
    /// `vorbis_find_page`: scans for the next page whose CRC matches and
    /// leaves the file at its start. Returns where it ends and whether it is
    /// the last page.
    pub fn find_page(&mut self) -> Option<(u32, bool)> {
        let f = &mut self.r;
        loop {
            if f.eof {
                return None;
            }
            if f.get8() != 0x4f {
                continue;
            }
            // page header candidate
            let retry_loc = f.file_offset();
            // check if we're off the end of a file_section stream
            if retry_loc.wrapping_sub(25) > f.stream_len {
                return None;
            }
            // check the rest of the header
            let mut i = 1;
            while i < 4 && f.get8() == OGG_PAGE_HEADER[i] {
                i += 1;
            }
            if f.eof {
                return None;
            }
            if i == 4 {
                let mut header = [0_u8; 27];
                header[..4].copy_from_slice(&OGG_PAGE_HEADER);
                for byte in &mut header[4..] {
                    *byte = f.get8();
                }
                if f.eof {
                    return None;
                }
                if header[4] == 0 {
                    let goal = u32::from_le_bytes([header[22], header[23], header[24], header[25]]);
                    header[22..26].fill(0);
                    let mut crc = header.iter().fold(0, |crc, &b| crc32_update(crc, b));
                    let mut len = 0_u32;
                    for _ in 0..header[26] {
                        let s = f.get8();
                        crc = crc32_update(crc, s);
                        len += u32::from(s);
                    }
                    if len != 0 && f.eof {
                        return None;
                    }
                    for _ in 0..len {
                        crc = crc32_update(crc, f.get8());
                    }
                    // finished parsing probable page
                    if crc == goal {
                        let end = f.file_offset();
                        let last = header[5] & 0x04 != 0;
                        f.set_file_offset(retry_loc.wrapping_sub(1));
                        return Some((end, last));
                    }
                }
            }
            // not a valid page, so rewind and look for next one
            f.set_file_offset(retry_loc);
        }
    }

    /// `stb_vorbis_stream_length_in_samples`, from the granule position of
    /// the last page (cached, as stb does); 0 if unknown.
    pub fn stream_length_in_samples(&mut self) -> u32 {
        if self.total_samples == 0 {
            // first, store the current decode position so we can restore it
            let restore_offset = self.r.file_offset();
            // now we want to seek back 64K from the end
            let stream_len = self.r.stream_len;
            let previous_safe =
                if stream_len >= 65536 && stream_len - 65536 >= self.first_audio_page_offset {
                    stream_len - 65536
                } else {
                    self.first_audio_page_offset
                };
            self.r.set_file_offset(previous_safe);
            match self.find_page() {
                // if we can't find a page, we're hosed!
                None => self.total_samples = SAMPLE_UNKNOWN,
                Some((mut end, mut last)) => {
                    // check if there are more pages
                    let mut last_page_loc = self.r.file_offset();
                    // stop when the last_page flag is set, not when we reach eof
                    while !last {
                        self.r.set_file_offset(end);
                        let Some((e, l)) = self.find_page() else {
                            break;
                        };
                        (end, last) = (e, l);
                        last_page_loc = self.r.file_offset();
                    }
                    self.r.set_file_offset(last_page_loc);
                    // parse the header
                    let mut header = [0_u8; 6];
                    self.r.getn(&mut header);
                    // extract the absolute granule position
                    let mut lo = self.r.get32();
                    let hi = self.r.get32();
                    if lo == 0xffff_ffff && hi == 0xffff_ffff {
                        self.total_samples = SAMPLE_UNKNOWN;
                    } else {
                        if hi != 0 {
                            lo = 0xffff_fffe; // saturate
                        }
                        self.total_samples = lo;
                        self.p_last = ProbedPage {
                            page_start: last_page_loc,
                            page_end: end,
                            last_decoded_sample: lo,
                        };
                    }
                }
            }
            self.r.set_file_offset(restore_offset);
        }
        if self.total_samples == SAMPLE_UNKNOWN {
            0
        } else {
            self.total_samples
        }
    }
}
