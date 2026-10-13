use std::sync::Arc;

use solouder_core::{
    Clipper, Mixer, MixerConfig, SAMPLE_GRANULARITY, Stream, StreamReader, StreamSource,
};

use super::WavStream;
use crate::{DecodeLimits, decode_wav};

const BLOCK: usize = SAMPLE_GRANULARITY as usize;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/wav/",
            $name
        ))
        .as_slice()
    };
}

const GOOD: [&[u8]; 8] = [
    fixture!("s16_stereo_44100.wav"),
    fixture!("ext_f32_7.1_44100.wav"),
    fixture!("ext_s24in32_stereo_32000.wav"),
    fixture!("chunks_s16_stereo_11025.wav"),
    fixture!("alaw_mono_8000.wav"),
    fixture!("f64_mono_96000.wav"),
    fixture!("u8_mono_8000.wav"),
    fixture!("short_data_s16_mono_22050.wav"),
];

/// Reads a whole stream in blocks of `chunk` frames, planar.
fn read_all(
    reader: &mut dyn StreamReader,
    channels: usize,
    frames: usize,
    chunk: usize,
) -> Vec<f32> {
    let mut out = vec![0.0; channels * frames];
    let mut block = vec![f32::NAN; channels * BLOCK];
    let mut at = 0;
    loop {
        let got = reader.read(&mut block, BLOCK, chunk);
        for c in 0..channels {
            out[c * frames + at..][..got].copy_from_slice(&block[c * BLOCK..][..got]);
        }
        at += got;
        if got < chunk {
            break;
        }
    }
    assert_eq!(at, frames);
    out
}

#[test]
fn streams_decode_like_resident_files() {
    for bytes in GOOD {
        let pcm = decode_wav(bytes, &DecodeLimits::default()).unwrap();
        let stream = WavStream::from_bytes(bytes.to_vec()).unwrap();
        let format = stream.format();
        assert_eq!(
            (format.channels, format.sample_rate, format.frames),
            (pcm.channels(), pcm.sample_rate(), pcm.frames())
        );
        let (channels, frames) = (pcm.channels() as usize, pcm.frames() as usize);
        for chunk in [1, 7, BLOCK] {
            let mut reader = stream.open().unwrap();
            let read = read_all(reader.as_mut(), channels, frames, chunk);
            for c in 0..channels {
                assert_eq!(&read[c * frames..][..frames], pcm.channel(c as u32));
            }
        }
    }
}

#[test]
fn rewind_and_skip_land_where_decoding_does() {
    let bytes = fixture!("s16_stereo_44100.wav");
    let pcm = decode_wav(bytes, &DecodeLimits::default()).unwrap();
    let stream = WavStream::from_bytes(bytes.to_vec()).unwrap();
    let mut reader = stream.open().unwrap();
    let mut block = vec![0.0; 2 * BLOCK];
    assert_eq!(reader.skip(100), 100);
    assert_eq!(reader.read(&mut block, BLOCK, 3), 3);
    assert_eq!(block[..3], pcm.channel(0)[100..103]);
    assert_eq!(block[BLOCK..BLOCK + 3], pcm.channel(1)[100..103]);
    assert!(reader.rewind());
    assert_eq!(reader.read(&mut block, BLOCK, 1), 1);
    assert_eq!(block[0], pcm.channel(0)[0]);
    let left = u64::from(pcm.frames()) - 1;
    assert_eq!(reader.skip(left + 50), left);
    assert_eq!(reader.read(&mut block, BLOCK, 1), 0);
}

fn mixer() -> Mixer {
    Mixer::new(MixerConfig {
        clipper: Clipper::Hard,
        ..MixerConfig::default()
    })
    .unwrap()
}

/// A mono float WAV whose data chunk declares `declared` frames and holds
/// `present`.
fn cut_wav(declared: u32, present: u32) -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + declared * 4).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 0x0001_0003, 44100, 44100 * 4, 0x0020_0004] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(declared * 4).to_le_bytes());
    for _ in 0..present {
        out.extend_from_slice(&0.25f32.to_le_bytes());
    }
    out
}

/// Reference patch 0054: a stream cut off in its data chunk ends after the
/// frames present (SoLoud played silence forever).
#[test]
fn a_cut_off_stream_ends_after_its_frames() {
    let stream = Stream::new(Arc::new(
        WavStream::from_bytes(cut_wav(44100, 1000)).unwrap(),
    ))
    .unwrap();
    assert_eq!(stream.format().frames, 1000);
    let mut mixer = mixer();
    let h = mixer.play(&stream);
    let mut out = vec![0.0; 2 * 4096];
    mixer.mix_interleaved(&mut out);
    // The linear resampler starts from the silence before the first frame.
    assert!(out[2..2 * 900].iter().all(|&s| s > 0.1));
    assert!(!mixer.is_valid_voice_handle(h));
}

/// Reference patch 0053: a stream whose file is gone when it plays ends at
/// once (SoLoud crashed).
#[test]
fn a_stream_whose_file_is_gone_ends_at_once() {
    let path = std::env::temp_dir().join(format!("solouder_gone_{}.wav", std::process::id()));
    std::fs::write(&path, cut_wav(4410, 4410)).unwrap();
    let wav = WavStream::open(&path).unwrap();
    assert!(wav.open().is_some());
    std::fs::remove_file(&path).unwrap();
    assert!(wav.open().is_none());
    let stream = Stream::new(Arc::new(wav)).unwrap();
    let mut mixer = mixer();
    let h = mixer.play(&stream);
    let mut out = vec![1.0; 2 * 2048];
    mixer.mix_interleaved(&mut out);
    assert!(out.iter().all(|&s| s == 0.0));
    assert!(!mixer.is_valid_voice_handle(h));
}

#[test]
fn a_stream_file_plays_like_its_bytes() {
    let bytes = fixture!("ext_s16_5.1_48000.wav");
    let path = std::env::temp_dir().join(format!("solouder_file_{}.wav", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    let render = |source: WavStream| {
        let mut stream = Stream::new(Arc::new(source)).unwrap();
        stream.set_looping(true);
        let mut mixer = mixer();
        let h = mixer.play(&stream);
        let mut out = vec![0.0; 2 * 3000];
        mixer.mix_interleaved(&mut out[..2 * 1000]);
        mixer.seek(h, 0.01).unwrap();
        mixer.mix_interleaved(&mut out[2 * 1000..]);
        out
    };
    let from_file = render(WavStream::open(&path).unwrap());
    let from_memory = render(WavStream::load_to_memory(&path).unwrap());
    std::fs::remove_file(&path).unwrap();
    assert_eq!(from_file, from_memory);
    assert!(from_file.iter().any(|&s| s != 0.0));
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
    for round in 0..5_000 {
        let mut bytes = GOOD[round % GOOD.len()].to_vec();
        for _ in 0..1 + next() % 4 {
            let at = if next() % 4 == 0 {
                next() % bytes.len()
            } else {
                next() % 80
            };
            bytes[at] = next() as u8;
        }
        if next() % 3 == 0 {
            bytes.truncate(next() % bytes.len());
        }
        let Ok(stream) = WavStream::from_bytes(bytes) else {
            continue;
        };
        let mut reader = stream.open().unwrap();
        while reader.read(&mut block, BLOCK, BLOCK) == BLOCK {}
        reader.rewind();
        reader.skip(next() as u64);
        reader.read(&mut block, BLOCK, BLOCK);
    }
}
