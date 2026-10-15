// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! minimp3's frame decoder (`drmp3dec`, `drmp3dec_init` and
//! `drmp3dec_decode_frame`).

use super::bits::Bs;
use super::header::{
    HDR_SIZE, find_frame, frame_bytes, frame_samples, get_layer, hdr_at, hdr_compare, is_crc,
    is_mono, padding, sample_rate_hz, test_mpeg1,
};
use super::layer3::{self, SCF_AT, restore_reservoir, save_reservoir};
use super::layer12::{ScaleInfo, apply_scf_384, dequantize_granule, read_scale_info};
use super::side_info::{GrInfo, read_side_info};
use super::synth::synth_granule;

/// `DRMP3_MAX_BITRESERVOIR_BYTES`.
pub(super) const MAX_BITRESERVOIR_BYTES: i32 = 511;
/// `DRMP3_MAX_L3_FRAME_PAYLOAD_BYTES`.
const MAX_L3_FRAME_PAYLOAD_BYTES: usize = 2304;
/// `DRMP3_MAX_SAMPLES_PER_FRAME`: 1152 frames of two channels.
pub(super) const MAX_SAMPLES_PER_FRAME: usize = 1152 * 2;

/// `drmp3dec_frame_info`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FrameInfo {
    /// The bytes the call consumed: skipped bytes and the frame.
    pub frame_bytes: i32,
    pub channels: u32,
    pub hz: u32,
}

/// `drmp3dec_scratch`. In C it lives on the stack of
/// `drmp3dec_decode_frame`, and some damaged frames read parts of it that
/// the frame did not write (scale factors copied for the first granule,
/// lines past the main data). Here it is kept with the decoder and cleared
/// with it, so such frames decode the same way every time.
pub(super) struct Scratch {
    pub bs: Bs,
    pub maindata: [u8; MAX_BITRESERVOIR_BYTES as usize + MAX_L3_FRAME_PAYLOAD_BYTES],
    pub gr_info: [GrInfo; 4],
    /// `grbuf[2][576]` followed by `scf[40]` (see [`SCF_AT`]).
    pub grbuf: [f32; SCF_AT + 40],
    pub syn: [f32; 33 * 64],
    pub ist_pos: [[u8; 39]; 2],
}

/// `drmp3dec`: the state kept between frames.
pub(super) struct Decoder {
    mdct_overlap: [[f32; 9 * 32]; 2],
    qmf_state: [f32; 15 * 64],
    reserv: i32,
    free_format_bytes: i32,
    pub header: [u8; 4],
    reserv_buf: [u8; 511],
    scratch: Scratch,
}

impl Decoder {
    /// A cleared decoder (`DRMP3_ZERO_OBJECT`), boxed: it is about 20 KiB.
    pub fn new() -> Box<Self> {
        Box::new(Self {
            mdct_overlap: [[0.0; 9 * 32]; 2],
            qmf_state: [0.0; 15 * 64],
            reserv: 0,
            free_format_bytes: 0,
            header: [0; 4],
            reserv_buf: [0; 511],
            scratch: Scratch {
                bs: Bs::default(),
                maindata: [0; MAX_BITRESERVOIR_BYTES as usize + MAX_L3_FRAME_PAYLOAD_BYTES],
                gr_info: [GrInfo::default(); 4],
                grbuf: [0.0; SCF_AT + 40],
                syn: [0.0; 33 * 64],
                ist_pos: [[0; 39]; 2],
            },
        })
    }

    /// `drmp3dec_init`: the next frame is searched for from scratch.
    pub fn init(&mut self) {
        self.header[0] = 0;
    }

    /// `DRMP3_ZERO_MEMORY(dec, sizeof(drmp3dec))`, and the scratch.
    fn clear(&mut self) {
        self.mdct_overlap = [[0.0; 9 * 32]; 2];
        self.qmf_state = [0.0; 15 * 64];
        self.reserv = 0;
        self.free_format_bytes = 0;
        self.header = [0; 4];
        self.reserv_buf = [0; 511];
        let s = &mut self.scratch;
        s.bs = Bs::default();
        s.maindata.fill(0);
        s.gr_info = [GrInfo::default(); 4];
        s.grbuf.fill(0.0);
        s.syn.fill(0.0);
        s.ist_pos = [[0; 39]; 2];
    }

    /// `drmp3dec_decode_frame`: finds and decodes the next frame of `mp3`
    /// into `pcm` (interleaved), or with `pcm` `None` only checks it
    /// (layer 3 still updates its bit reservoir). Returns the frames
    /// decoded, 0 when the frame could not be decoded or none was found;
    /// `frame_bytes` says how much of `mp3` was used either way.
    pub fn decode_frame(
        &mut self,
        mp3: &[u8],
        mut pcm: Option<&mut [f32; MAX_SAMPLES_PER_FRAME]>,
    ) -> (u32, FrameInfo) {
        let mp3_bytes = i32::try_from(mp3.len()).unwrap_or(i32::MAX);
        let mut info = FrameInfo::default();
        let mut i = 0;
        let mut frame_size = 0;
        let first = hdr_at(mp3, 0);

        if mp3_bytes > 4 && self.header[0] == 0xff && hdr_compare(&self.header, &first) {
            frame_size = frame_bytes(&first, self.free_format_bytes) + padding(&first);
            if frame_size != mp3_bytes
                && (i64::from(frame_size) + i64::from(HDR_SIZE) > i64::from(mp3_bytes)
                    || !hdr_compare(&first, &hdr_at(mp3, frame_size)))
            {
                frame_size = 0;
            }
        }
        if frame_size == 0 {
            self.clear();
            (i, frame_size) = find_frame(mp3, mp3_bytes, &mut self.free_format_bytes);
            if frame_size == 0 || i64::from(i) + i64::from(frame_size) > i64::from(mp3_bytes) {
                info.frame_bytes = i;
                return (0, info);
            }
        }

        let hdr = hdr_at(mp3, i);
        self.header = hdr;
        info.frame_bytes = i + frame_size;
        info.channels = if is_mono(&hdr) { 1 } else { 2 };
        info.hz = sample_rate_hz(&hdr);
        let layer = 4 - get_layer(&hdr);

        let start = (i + HDR_SIZE) as usize;
        let end = (i + frame_size) as usize;
        let frame = mp3.get(start..end).unwrap_or_default();
        let mut bs = Bs::new(frame_size - HDR_SIZE);
        if is_crc(&hdr) {
            bs.get_bits(frame, 16);
        }
        let nch = info.channels as usize;
        let mut success = true;

        if layer == 3 {
            let s = &mut self.scratch;
            let main_data_begin = read_side_info(&mut bs, frame, &mut s.gr_info, &hdr);
            if main_data_begin < 0 || bs.pos > bs.limit {
                self.init();
                return (0, info);
            }
            success = restore_reservoir(
                self.reserv,
                &self.reserv_buf,
                &bs,
                frame,
                s,
                main_data_begin,
            );
            if let Some(pcm) = pcm.as_deref_mut().filter(|_| success) {
                let granules = if test_mpeg1(&hdr) { 2 } else { 1 };
                for igr in 0..granules {
                    s.grbuf[..SCF_AT].fill(0.0);
                    layer3::decode(&hdr, &mut self.mdct_overlap, s, igr * nch, nch);
                    let at = 576 * nch * igr;
                    synth_granule(
                        &mut self.qmf_state,
                        &mut s.grbuf,
                        18,
                        nch,
                        pcm,
                        at,
                        &mut s.syn,
                    );
                }
            }
            save_reservoir(&mut self.reserv, &mut self.reserv_buf, s);
        } else {
            let Some(pcm) = pcm else {
                return (frame_samples(&hdr), info);
            };
            let s = &mut self.scratch;
            let mut sci = ScaleInfo::default();
            read_scale_info(&hdr, &mut bs, frame, &mut sci);

            s.grbuf[..SCF_AT].fill(0.0);
            let mut i = 0;
            let mut pcm_at = 0;
            for igr in 0..3 {
                i +=
                    dequantize_granule(&mut s.grbuf, i, &mut bs, frame, &sci, (layer | 1) as usize);
                if i == 12 {
                    i = 0;
                    apply_scf_384(&sci, igr, &mut s.grbuf);
                    synth_granule(
                        &mut self.qmf_state,
                        &mut s.grbuf,
                        12,
                        nch,
                        pcm,
                        pcm_at,
                        &mut s.syn,
                    );
                    s.grbuf[..SCF_AT].fill(0.0);
                    pcm_at += 384 * nch;
                }
                if bs.pos > bs.limit {
                    self.init();
                    return (0, info);
                }
            }
        }

        let samples = if success {
            frame_samples(&self.header)
        } else {
            0
        };
        (samples, info)
    }
}
