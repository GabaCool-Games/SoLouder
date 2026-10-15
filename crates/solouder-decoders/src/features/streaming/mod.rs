//! Streamed audio: files decoded a block at a time while they play
//! (SoLoud's `WavStream`), through `solouder_core`'s
//! [`StreamSource`](solouder_core::StreamSource) traits.

mod flac_reader;
mod kind;
mod mp3_reader;
mod ogg_reader;
mod wav_reader;
mod wav_stream;

pub use wav_stream::WavStream;

#[cfg(test)]
#[path = "flac_stream_tests.rs"]
mod flac_stream_tests;
#[cfg(test)]
#[path = "lossy_stream_tests.rs"]
mod lossy_stream_tests;
#[cfg(test)]
#[path = "wav_stream_tests.rs"]
mod wav_stream_tests;
