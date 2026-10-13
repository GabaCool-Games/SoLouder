use std::sync::Arc;

use solouder_core::{
    Clipper, Mixer, MixerConfig, SAMPLE_GRANULARITY, Stream, StreamReader, StreamSource,
};

use super::WavStream;
use crate::{DecodeLimits, decode_flac};

const BLOCK: usize = SAMPLE_GRANULARITY as usize;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/flac/",
            $name
        ))
        .as_slice()
    };
}

const GOOD: [&[u8]; 11] = [
    fixture!("s16_stereo_44100.flac"),
    fixture!("s24_mono_48000.flac"),
    fixture!("s8_quad_8000.flac"),
    fixture!("s12_5.1_22050.flac"),
    fixture!("s20_stereo_96000_variable.flac"),
    fixture!("s16_7.1_32000_metadata.flac"),
    fixture!("s32_mono_44100.flac"),
    fixture!("s28_stereo_44100.flac"),
    fixture!("s32_stereo_44100.flac"),
    fixture!("s8_mono_8000_long.flac"),
    fixture!("s16_mono_44100_variable_equal.flac"),
];

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

fn decoded(bytes: &[u8]) -> solouder_core::Pcm {
    decode_flac(bytes, &DecodeLimits::default()).unwrap()
}

#[test]
fn streams_decode_like_resident_files() {
    for bytes in GOOD {
        let pcm = decoded(bytes);
        let stream = WavStream::from_bytes(bytes.to_vec()).unwrap();
        let format = stream.format();
        assert_eq!(
            (format.channels, format.sample_rate, format.frames),
            (pcm.channels(), pcm.sample_rate(), pcm.frames())
        );
        let (channels, frames) = (pcm.channels() as usize, pcm.frames() as usize);
        for chunk in [1, 7, BLOCK] {
            let mut reader = stream.open().unwrap();
            let (read, count) = read_rest(reader.as_mut(), channels, frames, chunk);
            assert_eq!(count, frames);
            for c in 0..channels {
                assert_eq!(&read[c * frames..][..frames], pcm.channel(c as u32));
            }
        }
    }
}

#[test]
fn skips_land_where_decoding_does() {
    for bytes in GOOD {
        let pcm = decoded(bytes);
        let stream = WavStream::from_bytes(bytes.to_vec()).unwrap();
        let (channels, frames) = (pcm.channels() as usize, pcm.frames() as u64);
        let mut reader = stream.open().unwrap();
        let mut block = vec![0.0; channels * BLOCK];
        // Short and long skips, forwards from wherever the last one landed.
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

#[test]
fn damaged_files_stream_like_they_decode() {
    for bytes in [
        fixture!("bad_crc_s16_mono_44100.flac"),
        fixture!("bad_crc_runs_s16_mono_44100.flac"),
        fixture!("bad_crc8_s16_mono_44100.flac"),
        fixture!("bad_overlap_s16_mono_44100.flac"),
        fixture!("short_data_s16_mono_44100.flac"),
        fixture!("long_claim_s16_mono_44100.flac"),
        fixture!("bad_overflow_s16_stereo_44100.flac"),
    ] {
        let pcm = decoded(bytes);
        let stream = WavStream::from_bytes(bytes.to_vec()).unwrap();
        let (channels, frames) = (pcm.channels() as usize, pcm.frames() as usize);
        let mut reader = stream.open().unwrap();
        let (read, count) = read_rest(reader.as_mut(), channels, frames, BLOCK);
        assert_eq!(count, frames);
        for c in 0..channels {
            assert_eq!(&read[c * frames..][..frames], pcm.channel(c as u32));
        }
        // Skipping counts the silent frames as decoding does.
        let mut reader = stream.open().unwrap();
        assert_eq!(reader.skip(frames as u64 - 1), frames as u64 - 1);
        let mut block = vec![0.0; channels * BLOCK];
        assert_eq!(reader.read(&mut block, BLOCK, BLOCK), 1);
        assert_eq!(block[0], pcm.channel(0)[frames - 1]);
    }
    let claim = WavStream::from_bytes(fixture!("long_claim_s16_mono_44100.flac").to_vec());
    assert_eq!(claim.unwrap().format().frames, 10_000);
}

fn mixer() -> Mixer {
    Mixer::new(MixerConfig {
        clipper: Clipper::Hard,
        ..MixerConfig::default()
    })
    .unwrap()
}

#[test]
fn a_stream_of_unknown_length_ends_at_once() {
    let wav = WavStream::from_bytes(fixture!("unknown_length_s16_mono_44100.flac").to_vec());
    let stream = Stream::new(Arc::new(wav.unwrap())).unwrap();
    assert_eq!(stream.format().frames, 0);
    let mut mixer = mixer();
    let h = mixer.play(&stream);
    let mut out = vec![0.0; 2 * 2048];
    mixer.mix_interleaved(&mut out);
    assert!(out.iter().all(|&s| s == 0.0));
    assert!(!mixer.is_valid_voice_handle(h));
}

#[test]
fn bad_headers_are_rejected() {
    for bytes in [
        fixture!("bad_cut_header.flac"),
        fixture!("bad_zero_rate.flac"),
        b"fLaC".as_slice(),
    ] {
        assert!(WavStream::from_bytes(bytes.to_vec()).is_err());
    }
}

#[test]
fn a_stream_file_plays_like_its_bytes_until_it_is_gone() {
    let bytes = fixture!("s8_mono_8000_long.flac");
    let path = std::env::temp_dir().join(format!("solouder_file_{}.flac", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    let render = |source: WavStream| {
        let mut stream = Stream::new(Arc::new(source)).unwrap();
        stream.set_looping(true);
        let mut mixer = mixer();
        let h = mixer.play(&stream);
        let mut out = vec![0.0; 2 * 3000];
        mixer.mix_interleaved(&mut out[..2 * 1000]);
        mixer.seek(h, 3.5).unwrap();
        mixer.mix_interleaved(&mut out[2 * 1000..]);
        out
    };
    let wav = WavStream::open(&path).unwrap();
    let from_file = render(WavStream::open(&path).unwrap());
    let from_memory = render(WavStream::load_to_memory(&path).unwrap());
    assert_eq!(from_file, from_memory);
    assert!(from_file.iter().any(|&s| s != 0.0));

    // A voice keeps the file it opened: deleted while playing, it loops on.
    let mut stream = Stream::new(Arc::new(wav)).unwrap();
    stream.set_looping(true);
    let mut mixer = mixer();
    let h = mixer.play(&stream);
    let mut out = vec![0.0; 2 * 1000];
    mixer.mix_interleaved(&mut out);
    std::fs::remove_file(&path).unwrap();
    mixer.seek(h, 4.5).unwrap();
    let mut out = vec![0.0; 2 * 8192];
    mixer.mix_interleaved(&mut out);
    assert!(mixer.is_valid_voice_handle(h));
    assert!(out[2 * 4096..].iter().any(|&s| s != 0.0));
    assert!(WavStream::open(&path).is_err());
}

#[test]
fn mutated_streams_never_panic() {
    let mut state = 0x1234_5678_u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as usize
    };
    let mut block = vec![0.0; 8 * BLOCK];
    for round in 0..2_000 {
        let mut bytes = GOOD[round % GOOD.len()].to_vec();
        for _ in 0..1 + next() % 4 {
            let at = if next() % 4 == 0 {
                next() % bytes.len()
            } else {
                next() % 400
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
        while reader.read(&mut block, BLOCK, BLOCK) == BLOCK && reads < 10_000 {
            reads += 1;
        }
        reader.rewind();
        reader.skip(next() as u64 % 100_000);
        reader.read(&mut block, BLOCK, BLOCK);
    }
}
