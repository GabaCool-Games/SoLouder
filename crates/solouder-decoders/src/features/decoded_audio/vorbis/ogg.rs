// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! Ogg pages, packets and segments, and the bit reader on top of them:
//! stb_vorbis's `get8` to `prep_huffman`, with its state.

use super::source::Source;

/// End of packet, as `get8_packet` returns it.
pub(super) const EOP: i32 = -1;
/// `valid_bits` once a read ran past the end of the packet.
pub(super) const INVALID_BITS: i32 = -1;

pub(super) const PAGEFLAG_CONTINUED_PACKET: u8 = 1;
pub(super) const PAGEFLAG_FIRST_PAGE: u8 = 2;
pub(super) const PAGEFLAG_LAST_PAGE: u8 = 4;

/// The errors of stb_vorbis that SoLouder tells apart (`STBVorbisError`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VorbisError {
    None,
    UnexpectedEof,
    InvalidSetup,
    InvalidStream,
    MissingCapturePattern,
    InvalidStreamStructureVersion,
    ContinuedPacketFlagInvalid,
    InvalidFirstPage,
    OggSkeletonNotSupported,
    TooManyChannels,
    FeatureNotSupported,
    OutOfMem,
    /// Not in stb: `MissingCapturePattern` on the first page.
    NotOgg,
    /// Not in stb: a setup header asking for more memory than SoLouder
    /// gives one (see `setup::SETUP_BUDGET`).
    SetupTooLarge,
}

/// `ProbedPage`: where a page starts and ends, and its granule position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ProbedPage {
    pub page_start: u32,
    pub page_end: u32,
    pub last_decoded_sample: u32,
}

/// The input and the page, packet and bit state of `struct stb_vorbis`.
pub(super) struct Reader<S> {
    pub src: S,
    pub eof: bool,
    pub error: VorbisError,
    pub stream_len: u32,
    pub first_decode: bool,
    /// The page on which the first audio packet ends.
    pub p_first: ProbedPage,
    pub page_flag: u8,
    pub segment_count: i32,
    pub segments: [u8; 255],
    pub bytes_in_seg: u8,
    pub next_seg: i32,
    pub last_seg: bool,
    pub last_seg_which: i32,
    pub acc: u32,
    pub valid_bits: i32,
    pub packet_bytes: i32,
    pub end_seg_with_known_loc: i32,
    pub known_loc_for_packet: u32,
}

impl<S: Source> Reader<S> {
    /// `vorbis_init` for a source of `stream_len` bytes.
    pub fn new(src: S, stream_len: u32) -> Self {
        Self {
            src,
            eof: false,
            error: VorbisError::None,
            stream_len,
            first_decode: false,
            p_first: ProbedPage::default(),
            page_flag: 0,
            segment_count: 0,
            segments: [0; 255],
            bytes_in_seg: 0,
            next_seg: 0,
            last_seg: false,
            last_seg_which: 0,
            acc: 0,
            valid_bits: 0,
            packet_bytes: 0,
            end_seg_with_known_loc: 0,
            known_loc_for_packet: 0,
        }
    }

    /// `error`: records `e`; always false, as stb's returns 0.
    pub fn error(&mut self, e: VorbisError) -> bool {
        self.error = e;
        false
    }

    #[inline]
    pub fn get8(&mut self) -> u8 {
        match self.src.get8() {
            Some(byte) => byte,
            None => {
                self.eof = true;
                0
            }
        }
    }

    pub fn get32(&mut self) -> u32 {
        let mut x = u32::from(self.get8());
        x = x.wrapping_add(u32::from(self.get8()) << 8);
        x = x.wrapping_add(u32::from(self.get8()) << 16);
        x.wrapping_add(u32::from(self.get8()) << 24)
    }

    pub fn getn(&mut self, data: &mut [u8]) -> bool {
        if self.src.getn(data) {
            return true;
        }
        self.eof = true;
        false
    }

    pub fn skip(&mut self, n: i32) {
        if self.src.skip(n) {
            self.eof = true;
        }
    }

    pub fn set_file_offset(&mut self, loc: u32) -> bool {
        let (ok, eof) = self.src.set_offset(loc);
        self.eof = eof;
        ok
    }

    /// `stb_vorbis_get_file_offset`.
    pub fn file_offset(&mut self) -> u32 {
        self.src.offset()
    }

    fn capture_pattern(&mut self) -> bool {
        self.get8() == 0x4f && self.get8() == 0x67 && self.get8() == 0x67 && self.get8() == 0x53
    }

    pub fn start_page_no_capturepattern(&mut self) -> bool {
        if self.first_decode {
            self.p_first.page_start = self.file_offset().wrapping_sub(4);
        }
        // stream structure version
        if self.get8() != 0 {
            return self.error(VorbisError::InvalidStreamStructureVersion);
        }
        self.page_flag = self.get8();
        // absolute granule position
        let loc0 = self.get32();
        let loc1 = self.get32();
        // stream serial number, page sequence number, CRC32
        self.get32();
        self.get32();
        self.get32();
        self.segment_count = i32::from(self.get8());
        let count = self.segment_count as usize;
        let mut segments = self.segments;
        let read = self.getn(&mut segments[..count]);
        self.segments = segments;
        if !read {
            return self.error(VorbisError::UnexpectedEof);
        }
        // assume we _don't_ know any the sample position of any segments
        self.end_seg_with_known_loc = -2;
        if loc0 != !0 || loc1 != !0 {
            // the last segment of a packet that ends on this page
            if let Some(i) = self.segments[..count].iter().rposition(|&s| s < 255) {
                self.end_seg_with_known_loc = i as i32;
                self.known_loc_for_packet = loc0;
            }
        }
        if self.first_decode {
            let len = self.segments[..count]
                .iter()
                .fold(0_u32, |len, &s| len + u32::from(s))
                + 27
                + count as u32;
            self.p_first.page_end = self.p_first.page_start.wrapping_add(len);
            self.p_first.last_decoded_sample = loc0;
        }
        self.next_seg = 0;
        true
    }

    pub fn start_page(&mut self) -> bool {
        if !self.capture_pattern() {
            return self.error(VorbisError::MissingCapturePattern);
        }
        self.start_page_no_capturepattern()
    }

    pub fn start_packet(&mut self) -> bool {
        while self.next_seg == -1 {
            if !self.start_page() {
                return false;
            }
            if self.page_flag & PAGEFLAG_CONTINUED_PACKET != 0 {
                return self.error(VorbisError::ContinuedPacketFlagInvalid);
            }
        }
        self.last_seg = false;
        self.valid_bits = 0;
        self.packet_bytes = 0;
        self.bytes_in_seg = 0;
        true
    }

    pub fn maybe_start_packet(&mut self) -> bool {
        if self.next_seg == -1 {
            let x = self.get8();
            if self.eof {
                // EOF at page boundary is not an error!
                return false;
            }
            if x != 0x4f || self.get8() != 0x67 || self.get8() != 0x67 || self.get8() != 0x53 {
                return self.error(VorbisError::MissingCapturePattern);
            }
            if !self.start_page_no_capturepattern() {
                return false;
            }
            if self.page_flag & PAGEFLAG_CONTINUED_PACKET != 0 {
                // set up enough state that we can read this packet if we want
                self.last_seg = false;
                self.bytes_in_seg = 0;
                return self.error(VorbisError::ContinuedPacketFlagInvalid);
            }
        }
        self.start_packet()
    }

    pub fn next_segment(&mut self) -> i32 {
        if self.last_seg {
            return 0;
        }
        if self.next_seg == -1 {
            // in case start_page fails
            self.last_seg_which = self.segment_count - 1;
            if !self.start_page() {
                self.last_seg = true;
                return 0;
            }
            if self.page_flag & PAGEFLAG_CONTINUED_PACKET == 0 {
                self.error(VorbisError::ContinuedPacketFlagInvalid);
                return 0;
            }
        }
        // next_seg stays within the table (-1 is handled above).
        let Some(&len) = self.segments.get(self.next_seg as usize) else {
            self.last_seg = true;
            return 0;
        };
        self.next_seg += 1;
        if len < 255 {
            self.last_seg = true;
            self.last_seg_which = self.next_seg - 1;
        }
        if self.next_seg >= self.segment_count {
            self.next_seg = -1;
        }
        self.bytes_in_seg = len;
        i32::from(len)
    }

    #[inline]
    pub fn get8_packet_raw(&mut self) -> i32 {
        if self.bytes_in_seg == 0 && (self.last_seg || self.next_segment() == 0) {
            return EOP;
        }
        self.bytes_in_seg = self.bytes_in_seg.wrapping_sub(1);
        self.packet_bytes = self.packet_bytes.wrapping_add(1);
        i32::from(self.get8())
    }

    pub fn get8_packet(&mut self) -> i32 {
        let x = self.get8_packet_raw();
        self.valid_bits = 0;
        x
    }

    /// `get32_packet`: four bytes, each -1 at the end of the packet, summed
    /// as the C does.
    pub fn get32_packet(&mut self) -> i32 {
        let mut x = self.get8_packet() as u32;
        x = x.wrapping_add((self.get8_packet() << 8) as u32);
        x = x.wrapping_add((self.get8_packet() << 16) as u32);
        x.wrapping_add((self.get8_packet() as u32) << 24) as i32
    }

    pub fn flush_packet(&mut self) {
        while self.get8_packet_raw() != EOP {}
    }

    /// `get_bits`: `n` bits (0 to 32), least significant first; 0 once the
    /// packet has ended. Shifts by 32 behave as on x86, as the C's do.
    pub fn get_bits(&mut self, n: i32) -> u32 {
        if self.valid_bits < 0 {
            return 0;
        }
        if self.valid_bits < n {
            if n > 24 {
                // the accumulator technique below would not work correctly in this case
                let z = self.get_bits(24);
                return z.wrapping_add(self.get_bits(n - 24) << 24);
            }
            if self.valid_bits == 0 {
                self.acc = 0;
            }
            while self.valid_bits < n {
                let z = self.get8_packet_raw();
                if z == EOP {
                    self.valid_bits = INVALID_BITS;
                    return 0;
                }
                self.acc = self.acc.wrapping_add((z as u32) << self.valid_bits);
                self.valid_bits += 8;
            }
        }
        let z = self.acc & 1_u32.wrapping_shl(n as u32).wrapping_sub(1);
        self.acc = self.acc.wrapping_shr(n as u32);
        self.valid_bits -= n;
        z
    }

    /// `prep_huffman`: fills the accumulator to more than 24 bits without
    /// reading past the end of the packet.
    #[inline]
    pub fn prep_huffman(&mut self) {
        if self.valid_bits <= 24 {
            if self.valid_bits == 0 {
                self.acc = 0;
            }
            loop {
                if self.last_seg && self.bytes_in_seg == 0 {
                    return;
                }
                let z = self.get8_packet_raw();
                if z == EOP {
                    return;
                }
                // A negative count (after INVALID_BITS) shifts by 31, as on x86.
                self.acc = self
                    .acc
                    .wrapping_add((z as u32).wrapping_shl(self.valid_bits as u32));
                self.valid_bits += 8;
                if self.valid_bits > 24 {
                    return;
                }
            }
        }
    }
}
