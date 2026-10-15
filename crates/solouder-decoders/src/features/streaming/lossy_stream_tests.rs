use std::sync::Arc;

use solouder_core::{
    Clipper, Mixer, MixerConfig, Pcm, SAMPLE_GRANULARITY, Stream, StreamReader, StreamSource,
};

use super::WavStream;
use crate::{DecodeLimits, decode};

const BLOCK: usize = SAMPLE_GRANULARITY as usize;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/",
            $name
        ))
        .as_slice()
    };
}

const OGG: [&[u8]; 7] = [
    fixture!("ogg/vorbis_stereo_44100.ogg"),
    fixture!("ogg/vorbis_mono_22050_q0.ogg"),
    fixture!("ogg/vorbis_5.1_48000.ogg"),
    fixture!("ogg/vorbis_7.1_32000.ogg"),
    fixture!("ogg/vorbis_quad_16000_q10.ogg"),
    fixture!("ogg/vorbis_mono_8000_long.ogg"),
    fixture!("ogg/vorbis_short_mono_44100.ogg"),
];

const MP3: [&[u8]; 9] = [
    fixture!("mp3/mpeg1_stereo_44100_cbr128.mp3"),
    fixture!("mp3/mpeg1_stereo_48000_vbr_id3.mp3"),
    fixture!("mp3/mpeg1_mono_32000_noxing.mp3"),
    fixture!("mp3/mpeg2_mono_22050_vbr.mp3"),
    fixture!("mp3/mpeg2_stereo_16000.mp3"),
    fixture!("mp3/mpeg25_mono_8000.mp3"),
    fixture!("mp3/mpeg25_stereo_11025_long.mp3"),
    fixture!("mp3/layer2_stereo_44100.mp3"),
    fixture!("mp3/layer2_mono_24000.mp3"),
];

fn good() -> impl Iterator<Item = &'static [u8]> {
    OGG.into_iter().chain(MP3)
}

fn decoded(bytes: &[u8]) -> Pcm {
    decode(bytes, &DecodeLimits::default()).unwrap()
}

/// Reads the rest of a stream in calls of `chunk` frames, planar with
/// `frames` frames per channel; returns the frames read.
fn read_rest(
    reader: &mut dyn StreamReader,
    channels: usize,
    frames: usize,
    chunk: usize,
) -> (Vec<f32>, usize) {
    let mut out = vec![0.0; channels * frames];
    let mut block = vec![f32::NAN; channels * BLOCK];
    let mut at = 0;
    loop {
        let got = reader.read(&mut block, BLOCK, chunk.min(BLOCK));
        let room = frames.saturating_sub(at).min(got);
        if room > 0 {
            for c in 0..channels {
                out[c * frames + at..][..room].copy_from_slice(&block[c * BLOCK..][..room]);
            }
        }
        at += got;
        if got < chunk.min(BLOCK) {
            break;
        }
    }
    (out, at)
}

/// Reads exactly `frames` frames in calls of `chunk` frames, planar.
fn read_n(reader: &mut dyn StreamReader, channels: usize, frames: usize, chunk: usize) -> Vec<f32> {
    let mut out = vec![0.0; channels * frames];
    let mut block = vec![f32::NAN; channels * BLOCK];
    let mut at = 0;
    while at < frames {
        let want = chunk.min(BLOCK).min(frames - at);
        assert_eq!(reader.read(&mut block, BLOCK, want), want);
        for c in 0..channels {
            out[c * frames + at..][..want].copy_from_slice(&block[c * BLOCK..][..want]);
        }
        at += want;
    }
    out
}

fn assert_plays(read: &[f32], pcm: &Pcm, from: usize, what: &str) {
    let frames = read.len() / pcm.channels() as usize;
    for c in 0..pcm.channels() {
        let want = &pcm.channel(c)[from..][..frames];
        assert_eq!(&read[c as usize * frames..][..frames], want, "{what}");
    }
}

#[test]
fn streams_decode_like_resident_files() {
    for bytes in good() {
        let pcm = decoded(bytes);
        let stream = WavStream::from_bytes(bytes.to_vec()).unwrap();
        let format = stream.format();
        assert_eq!(
            (format.channels, format.sample_rate, format.frames),
            (pcm.channels(), pcm.sample_rate(), pcm.frames())
        );
        let (channels, frames) = (pcm.channels() as usize, pcm.frames() as usize);
        for chunk in [1, 7, 300, BLOCK] {
            let mut reader = stream.open().unwrap();
            let (read, count) = read_rest(reader.as_mut(), channels, frames, chunk);
            assert_eq!(count, frames, "chunk {chunk}");
            assert_plays(&read, &pcm, 0, "whole stream");
        }
    }
}

#[test]
fn skips_land_where_decoding_does() {
    for bytes in good() {
        let pcm = decoded(bytes);
        let stream = WavStream::from_bytes(bytes.to_vec()).unwrap();
        let (channels, frames) = (pcm.channels() as usize, pcm.frames() as u64);
        let mut reader = stream.open().unwrap();
        let mut block = vec![0.0; channels * BLOCK];
        // Short and long skips, forwards from wherever the last one landed,
        // inside a Vorbis or MPEG frame and across many.
        let mut at = 0;
        for skip in [100, 1, 4095, 20_000, 3, 11_000] {
            let landed = reader.skip(skip);
            assert_eq!(landed, skip.min(frames - at), "skip {skip} from {at}");
            at += landed;
            let got = reader.read(&mut block, BLOCK, 5);
            assert_eq!(got as u64, 5.min(frames - at));
            for c in 0..channels {
                let want = &pcm.channel(c as u32)[at as usize..][..got];
                assert_eq!(&block[c * BLOCK..][..got], want, "skip {skip} to {at}");
            }
            at += got as u64;
        }
        assert!(reader.rewind());
        assert_eq!(reader.read(&mut block, BLOCK, 1), 1);
        assert_eq!(block[0], pcm.channel(0)[0]);
    }
}

/// Reference patch 0058: a rewind forgets the Vorbis frame being read.
#[test]
fn a_rewound_ogg_stream_starts_over() {
    let bytes = OGG[0];
    let pcm = decoded(bytes);
    let mut reader = WavStream::from_bytes(bytes.to_vec())
        .unwrap()
        .open()
        .unwrap();
    let read = read_n(reader.as_mut(), 2, 1000, 300);
    assert_plays(&read, &pcm, 0, "before the rewind");
    assert!(reader.rewind());
    let read = read_n(reader.as_mut(), 2, 2048, BLOCK);
    assert_plays(&read, &pcm, 0, "after the rewind");
}

/// Reference patch 0058: a skip past the end ends the stream; one just
/// before it plays the last frames and no more.
#[test]
fn skipping_past_the_end_of_an_ogg_stream_ends_it() {
    let bytes = OGG[0];
    let pcm = decoded(bytes);
    let frames = u64::from(pcm.frames());
    let mut reader = WavStream::from_bytes(bytes.to_vec())
        .unwrap()
        .open()
        .unwrap();
    let mut block = vec![1.0; 2 * BLOCK];
    assert_eq!(reader.read(&mut block, BLOCK, 333), 333);
    assert_eq!(reader.skip(10 * frames), frames - 333);
    assert_eq!(reader.read(&mut block, BLOCK, BLOCK), 0);
    assert!(reader.rewind());
    let from = (44_100.0_f64 * 1.49).floor() as u64;
    assert_eq!(reader.skip(from), from);
    let left = (frames - from) as usize;
    let (read, count) = read_rest(reader.as_mut(), 2, left, BLOCK);
    assert_eq!(count, left);
    assert_plays(&read, &pcm, from as usize, "the last frames");
}

#[test]
fn damaged_files_stream_like_they_decode() {
    for bytes in [
        fixture!("ogg/bad_payload_stereo_44100.ogg"),
        fixture!("ogg/bad_crc_stereo_44100.ogg"),
        fixture!("mp3/bad_truncated_stereo_44100.mp3"),
        fixture!("mp3/bad_junk_prefix_stereo_44100.mp3"),
        fixture!("mp3/bad_payload_stereo_44100.mp3"),
    ] {
        let pcm = decoded(bytes);
        let stream = WavStream::from_bytes(bytes.to_vec()).unwrap();
        let (channels, frames) = (pcm.channels() as usize, pcm.frames() as usize);
        let mut reader = stream.open().unwrap();
        let (read, count) = read_rest(reader.as_mut(), channels, frames, BLOCK);
        // The audio may end before the length: a Vorbis packet that fails
        // to decode, or MPEG frames that fail after being counted. Decoding
        // leaves the rest silent.
        assert!(count <= frames);
        assert_plays(&read, &pcm, 0, "damaged file");
        for c in 0..channels {
            assert!(pcm.channel(c as u32)[count..].iter().all(|&s| s == 0.0));
        }
    }
}

#[test]
fn bad_files_are_rejected() {
    for bytes in [
        fixture!("ogg/bad_truncated_stereo_44100.ogg"),
        fixture!("ogg/bad_headers_only.ogg"),
        fixture!("ogg/bad_setup.ogg"),
        fixture!("ogg/bad_cut_header.ogg"),
        fixture!("mp3/bad_not_audio.mp3"),
        b"OggS".as_slice(),
        b"ID3".as_slice(),
    ] {
        assert!(WavStream::from_bytes(bytes.to_vec()).is_err());
        assert!(decode(bytes, &DecodeLimits::default()).is_err());
    }
}

fn mixer() -> Mixer {
    Mixer::new(MixerConfig {
        clipper: Clipper::Hard,
        ..MixerConfig::default()
    })
    .unwrap()
}

#[test]
fn stream_files_play_like_their_bytes() {
    for (bytes, name) in [(OGG[5], "long.ogg"), (MP3[6], "long.mp3")] {
        let path = std::env::temp_dir().join(format!("solouder_{}_{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let render = |source: WavStream| {
            let mut stream = Stream::new(Arc::new(source)).unwrap();
            stream.set_looping(true);
            stream.set_loop_point(0.5);
            let mut mixer = mixer();
            let h = mixer.play(&stream);
            let mut out = vec![0.0; 2 * 30_000];
            mixer.mix_interleaved(&mut out[..2 * 1000]);
            mixer.seek(h, 3.5).unwrap();
            mixer.mix_interleaved(&mut out[2 * 1000..2 * 2000]);
            mixer.seek(h, 0.25).unwrap();
            mixer.mix_interleaved(&mut out[2 * 2000..]);
            out
        };
        let from_file = render(WavStream::open(&path).unwrap());
        let from_memory = render(WavStream::load_to_memory(&path).unwrap());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(from_file, from_memory, "{name}");
        assert!(from_file.iter().any(|&s| s != 0.0));
    }
}

#[test]
fn mutated_streams_never_panic() {
    let mut state = 0x9E37_79B9_u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as usize
    };
    let files: Vec<&[u8]> = good().collect();
    let mut block = vec![0.0; 8 * BLOCK];
    for round in 0..600 {
        let mut bytes = files[round % files.len()].to_vec();
        for _ in 0..1 + next() % 6 {
            let at = if next() % 3 == 0 {
                next() % bytes.len()
            } else {
                next() % 5000.min(bytes.len())
            };
            bytes[at] = next() as u8;
        }
        if next() % 3 == 0 {
            bytes.truncate(next() % bytes.len());
        }
        let Ok(stream) = WavStream::from_bytes(bytes) else {
            continue;
        };
        let Some(mut reader) = stream.open() else {
            continue;
        };
        let mut reads = 0;
        while reader.read(&mut block, BLOCK, BLOCK) == BLOCK && reads < 2_000 {
            reads += 1;
        }
        reader.rewind();
        reader.skip(next() as u64 % 100_000);
        reader.read(&mut block, BLOCK, BLOCK);
    }
}
