// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! `start_decoder`: the identification, comment and setup headers.

use super::layout::{read_floors, read_mappings, read_modes, read_residues};
use super::ogg::{
    PAGEFLAG_CONTINUED_PACKET, PAGEFLAG_FIRST_PAGE, PAGEFLAG_LAST_PAGE, Reader, VorbisError,
};
use super::setup::{BlockTables, Budget, Setup, read_codebook, setup_malloc_ok};
use super::source::Source;
use super::tables::{compute_bitreverse, compute_twiddle_factors, compute_window};

/// `STB_VORBIS_MAX_CHANNELS`.
const MAX_CHANNELS: i32 = 16;

const VORBIS_PACKET_ID: i32 = 1;
const VORBIS_PACKET_COMMENT: i32 = 3;
const VORBIS_PACKET_SETUP: i32 = 5;

/// `VorbisError::None` stands for a failure stb records no error for.
fn failed(e: VorbisError) -> VorbisError {
    if e == VorbisError::None {
        VorbisError::InvalidSetup
    } else {
        e
    }
}

/// Reads `len` bytes of the packet and drops them (stb keeps the vendor
/// and comments, which SoLoud never reads). Past the end of the packet
/// nothing changes, so the loop stops there.
fn skip_packet_bytes<S: Source>(f: &mut Reader<S>, len: i32) {
    for _ in 0..len {
        if f.last_seg && f.bytes_in_seg == 0 {
            f.valid_bits = 0;
            return;
        }
        f.get8_packet();
    }
}

/// `setup_malloc(sizeof(char) * (len+1))` for a comment string.
fn string_fits(len: i32) -> bool {
    let sz = len.wrapping_add(1);
    sz >= 0 && setup_malloc_ok(sz as u64)
}

/// The identification header: channels, sample rate and blocksizes.
fn read_identification<S: Source>(f: &mut Reader<S>) -> Result<(i32, u32, [i32; 2]), VorbisError> {
    let mut header = [0_u8; 6];
    if !f.start_page() {
        return Err(match f.error {
            VorbisError::MissingCapturePattern => VorbisError::NotOgg,
            e => failed(e),
        });
    }
    // validate page flag
    if f.page_flag & PAGEFLAG_FIRST_PAGE == 0
        || f.page_flag & PAGEFLAG_LAST_PAGE != 0
        || f.page_flag & PAGEFLAG_CONTINUED_PACKET != 0
    {
        return Err(VorbisError::InvalidFirstPage);
    }
    // check for expected packet length
    if f.segment_count != 1 {
        return Err(VorbisError::InvalidFirstPage);
    }
    if f.segments[0] != 30 {
        // check for the Ogg skeleton fishead identifying header to refine our error
        if f.segments[0] == 64
            && f.getn(&mut header)
            && &header == b"fishea"
            && f.get8() == b'd'
            && f.get8() == 0
        {
            return Err(VorbisError::OggSkeletonNotSupported);
        }
        return Err(VorbisError::InvalidFirstPage);
    }
    // read packet
    if i32::from(f.get8()) != VORBIS_PACKET_ID {
        return Err(VorbisError::InvalidFirstPage);
    }
    if !f.getn(&mut header) {
        return Err(VorbisError::UnexpectedEof);
    }
    if &header != b"vorbis" {
        return Err(VorbisError::InvalidFirstPage);
    }
    // vorbis_version
    if f.get32() != 0 {
        return Err(VorbisError::InvalidFirstPage);
    }
    let channels = i32::from(f.get8());
    if channels == 0 {
        return Err(VorbisError::InvalidFirstPage);
    }
    if channels > MAX_CHANNELS {
        return Err(VorbisError::TooManyChannels);
    }
    let sample_rate = f.get32();
    if sample_rate == 0 {
        return Err(VorbisError::InvalidFirstPage);
    }
    // bitrate_maximum, bitrate_nominal, bitrate_minimum
    f.get32();
    f.get32();
    f.get32();
    let x = f.get8();
    let (log0, log1) = (i32::from(x & 15), i32::from(x >> 4));
    if !(6..=13).contains(&log0) || !(6..=13).contains(&log1) || log0 > log1 {
        return Err(VorbisError::InvalidSetup);
    }
    // framing_flag
    if f.get8() & 1 == 0 {
        return Err(VorbisError::InvalidFirstPage);
    }
    Ok((channels, sample_rate, [1 << log0, 1 << log1]))
}

/// The comment header, read and dropped, and the rest of its packet.
fn read_comments<S: Source>(f: &mut Reader<S>) -> Result<(), VorbisError> {
    if !f.start_page() || !f.start_packet() || f.next_segment() == 0 {
        return Err(failed(f.error));
    }
    if f.get8_packet() != VORBIS_PACKET_COMMENT {
        return Err(VorbisError::InvalidSetup);
    }
    let header: [u8; 6] = std::array::from_fn(|_| f.get8_packet() as u8);
    if &header != b"vorbis" {
        return Err(VorbisError::InvalidSetup);
    }
    // file vendor
    let len = f.get32_packet();
    if !string_fits(len) {
        return Err(VorbisError::OutOfMem);
    }
    skip_packet_bytes(f, len);
    // user comments
    let comment_list_length = f.get32_packet();
    if comment_list_length > 0 {
        // setup_malloc(sizeof(char*) * n), through int
        let sz = (8 * comment_list_length as u64) as u32 as i32;
        if sz < 0 || !setup_malloc_ok(sz as u64) {
            return Err(VorbisError::OutOfMem);
        }
    }
    for _ in 0..comment_list_length.max(0) {
        let len = f.get32_packet();
        if !string_fits(len) {
            return Err(VorbisError::OutOfMem);
        }
        skip_packet_bytes(f, len);
    }
    // framing_flag
    if f.get8_packet() as u8 & 1 == 0 {
        return Err(VorbisError::InvalidSetup);
    }
    let rest = i32::from(f.bytes_in_seg);
    f.skip(rest);
    f.bytes_in_seg = 0;
    loop {
        let len = f.next_segment();
        f.skip(len);
        f.bytes_in_seg = 0;
        if len == 0 {
            return Ok(());
        }
    }
}

/// `init_blocksize`.
fn init_blocksize(n: i32) -> BlockTables {
    let n = n.max(64);
    let (n2, n4, n8) = ((n >> 1) as usize, (n >> 2) as usize, (n >> 3) as usize);
    let mut t = BlockTables {
        a: vec![0.0; n2],
        b: vec![0.0; n2],
        c: vec![0.0; n4],
        window: vec![0.0; n2],
        bit_reverse: vec![0; n8],
    };
    compute_twiddle_factors(n, &mut t.a, &mut t.b, &mut t.c);
    compute_window(n, &mut t.window);
    compute_bitreverse(n, &mut t.bit_reverse);
    t
}

/// `start_decoder`: reads the three headers. Returns the setup and
/// `first_audio_page_offset`.
pub(super) fn start_decoder<S: Source>(f: &mut Reader<S>) -> Result<(Setup, u32), VorbisError> {
    // first page, first packet
    f.first_decode = true;
    let (channels, sample_rate, blocksize) = read_identification(f)?;
    // second packet!
    read_comments(f)?;
    // third packet!
    if !f.start_packet() {
        return Err(failed(f.error));
    }
    if f.get8_packet() != VORBIS_PACKET_SETUP {
        return Err(VorbisError::InvalidSetup);
    }
    let header: [u8; 6] = std::array::from_fn(|_| f.get8_packet() as u8);
    if &header != b"vorbis" {
        return Err(VorbisError::InvalidSetup);
    }

    let mut budget = Budget(0);
    // codebooks
    let codebook_count = f.get_bits(8) + 1;
    let mut codebooks = Vec::with_capacity(codebook_count as usize);
    for _ in 0..codebook_count {
        codebooks.push(read_codebook(f, &mut budget)?);
    }
    // time domain transfers (notused)
    let x = (f.get_bits(6) + 1) as u8;
    for _ in 0..x {
        if f.get_bits(16) != 0 {
            return Err(VorbisError::InvalidSetup);
        }
    }
    let (floors, longest_floorlist) = read_floors(f, codebooks.len())?;
    let residues = read_residues(f, &codebooks, &mut budget)?;
    let mappings = read_mappings(f, channels, floors.len(), residues.len())?;
    let modes = read_modes(f, mappings.len())?;

    f.flush_packet();

    let tables = [init_blocksize(blocksize[0]), init_blocksize(blocksize[1])];
    let setup = Setup {
        channels,
        sample_rate,
        blocksize,
        codebooks,
        floors,
        residues,
        mappings,
        modes,
        tables,
        longest_floorlist,
    };
    // stb_vorbis_seek_start expects first_audio_page_offset to point to a
    // page without PAGEFLAG_continued_packet
    let first_audio_page_offset = if f.next_seg == -1 { f.file_offset() } else { 0 };
    Ok((setup, first_audio_page_offset))
}
