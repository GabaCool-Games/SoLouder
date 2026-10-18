use std::io::{self, Cursor};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use solouder_core::{SAMPLE_GRANULARITY, StreamSource};

use super::{StreamFile, StreamInput, WavStream};

const BLOCK: usize = SAMPLE_GRANULARITY as usize;

const FILES: [&[u8]; 4] = [
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/wav/s16_stereo_44100.wav"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/flac/s16_stereo_44100.flac"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ogg/vorbis_stereo_44100.ogg"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/mp3/mpeg1_stereo_44100_cbr128.mp3"
    )),
];

/// Bytes behind a caller's own file type, counting how often it is opened.
#[derive(Debug)]
struct Counted {
    bytes: Vec<u8>,
    opens: Arc<AtomicUsize>,
    fail_after: usize,
}

impl StreamFile for Counted {
    fn open(&self) -> io::Result<Box<dyn StreamInput>> {
        if self.opens.fetch_add(1, Ordering::Relaxed) >= self.fail_after {
            return Err(io::Error::other("gone"));
        }
        Ok(Box::new(Cursor::new(self.bytes.clone())))
    }
}

/// The first `blocks` blocks a fresh voice reads, planar per block.
fn first_blocks(stream: &WavStream, blocks: usize) -> Vec<f32> {
    let channels = stream.format().channels as usize;
    let mut reader = stream.open().unwrap();
    let mut out = Vec::new();
    let mut block = vec![0.0; channels * BLOCK];
    for _ in 0..blocks {
        let got = reader.read(&mut block, BLOCK, BLOCK);
        out.extend_from_slice(&block[..channels * BLOCK]);
        if got < BLOCK {
            break;
        }
    }
    out
}

#[test]
fn every_way_of_handing_over_a_file_streams_the_same() {
    for bytes in FILES {
        let direct = WavStream::from_bytes(bytes.to_vec()).unwrap();
        let expected = first_blocks(&direct, 6);
        let shared: Arc<[u8]> = bytes.into();
        let opens = Arc::new(AtomicUsize::new(0));
        let custom = WavStream::from_file(Counted {
            bytes: bytes.to_vec(),
            opens: Arc::clone(&opens),
            fail_after: usize::MAX,
        })
        .unwrap();
        for stream in [
            WavStream::from_reader(bytes).unwrap(),
            WavStream::from_file(shared).unwrap(),
            custom,
        ] {
            assert_eq!(stream.format(), direct.format());
            assert_eq!(first_blocks(&stream, 6), expected);
        }
        // Probing opened it once, the voice once more.
        assert_eq!(opens.load(Ordering::Relaxed), 2);
    }
}

#[test]
fn voices_read_their_own_cursor() {
    let bytes = FILES[0];
    let stream = WavStream::from_file(Counted {
        bytes: bytes.to_vec(),
        opens: Arc::default(),
        fail_after: usize::MAX,
    })
    .unwrap();
    let mut first = stream.open().unwrap();
    let mut block = vec![0.0; 2 * BLOCK];
    assert_eq!(first.read(&mut block, BLOCK, BLOCK), BLOCK);
    // A second voice starts at the start, whatever the first has read.
    assert_eq!(first_blocks(&stream, 1), first_blocks(&stream, 1));
    let mut again = vec![0.0; 2 * BLOCK];
    assert_eq!(first.read(&mut again, BLOCK, BLOCK), BLOCK);
    assert_ne!(block, again);
}

#[test]
fn files_that_fail_to_open_fail_to_load_or_end_the_voice() {
    let missing = std::path::PathBuf::from("no such file.wav");
    assert!(WavStream::from_file(missing).is_err());
    let never = Counted {
        bytes: FILES[0].to_vec(),
        opens: Arc::default(),
        fail_after: 0,
    };
    assert!(WavStream::from_file(never).is_err());
    let once = WavStream::from_file(Counted {
        bytes: FILES[0].to_vec(),
        opens: Arc::default(),
        fail_after: 1,
    })
    .unwrap();
    assert!(once.open().is_none());
    assert!(WavStream::from_reader(&b"RIFF"[..]).is_err());
}
