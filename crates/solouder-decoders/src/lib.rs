//! SoLouder decoders: turn encoded audio files into [`solouder_core::Pcm`],
//! or stream them while they play.
//!
//! Decoding happens outside the audio callback; the mixer only ever sees
//! validated, immutable PCM or a [`solouder_core::StreamReader`]. WAV, FLAC,
//! Ogg Vorbis and MPEG audio, resident and streamed; the FLAC, Vorbis and
//! MPEG decoders are ports of dr_flac, stb_vorbis and dr_mp3 that decode bit
//! for bit as SoLoud's builds of them do.

mod features;

pub use features::decoded_audio::{
    DecodeError, DecodeLimits, decode, decode_flac, decode_mp3, decode_ogg, decode_reader,
    decode_wav,
};
pub use features::streaming::{StreamFile, StreamInput, WavStream};
