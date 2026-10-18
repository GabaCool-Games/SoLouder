use solouder_core::GeneratorContext;
use solouder_core::dsp::waveform;

use super::instance::MonotoneInstance;
use super::*;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/monotone/",
            $name
        ))
        .as_slice()
    };
}

/// `frames` of a new voice at `rate`, in 512-frame reads.
fn render(song: &Monotone, frames: usize, rate: f32) -> Vec<f32> {
    let mut instance = song.instance();
    let context = GeneratorContext::new(false, rate);
    let mut out = vec![0.0; frames];
    for block in out.chunks_mut(512) {
        let len = block.len();
        assert_eq!(instance.get_audio(block, len, 512, &context), len);
        assert!(!instance.has_ended());
    }
    out
}

/// A Monotone song in the file layout: `tracks` tracks, one pattern
/// with `cells` (row, track, cell), played in `order`.
fn song_bytes(tracks: u8, cells: &[(usize, usize, u16)], order: &[u8]) -> Vec<u8> {
    let mut out = b"\x08MONOTONE".to_vec();
    out.extend_from_slice(&[0; 82]);
    out.extend_from_slice(&[1, 1, tracks, 2]);
    let mut list = [0xff; 256];
    list[..order.len()].copy_from_slice(order);
    out.extend_from_slice(&list);
    let mut pattern = vec![0_u16; 64 * usize::from(tracks)];
    for &(row, track, cell) in cells {
        pattern[row * usize::from(tracks) + track] = cell;
    }
    out.extend(pattern.iter().flat_map(|cell| cell.to_le_bytes()));
    out
}

fn cell(note: u16, effect: u16, data: u16) -> u16 {
    note << 9 | effect << 6 | data
}

#[test]
fn fixtures_load() {
    let demo = Monotone::from_bytes(fixture!("demo.mon")).unwrap();
    let song = demo.song();
    assert_eq!((song.patterns(), song.tracks()), (3, 4));
    assert_eq!(song.title(), b"SoLouder demo");
    assert_eq!(song.comment(), b"Every effect");
    assert_eq!(&song.order()[..5], &[0, 1, 2, 1, 0xff]);
    let long = Monotone::from_bytes(fixture!("long_title.mon")).unwrap();
    assert_eq!(long.song().title(), [b'T'; 40]);
    assert_eq!(long.song().comment(), b"Comment");
    let trailing = Monotone::from_bytes(fixture!("trailing.mon")).unwrap();
    assert_eq!(trailing.song().tracks(), 1);
    let twelve = Monotone::from_reader(fixture!("twelve.mon")).unwrap();
    assert_eq!(twelve.song().tracks(), 12);
}

#[test]
fn damaged_files_fail_with_their_reason() {
    use MonotoneError as E;
    type Case<'a> = (&'a [u8], fn(&MonotoneError) -> bool);
    let cases: [Case; 8] = [
        (fixture!("bad_magic.mon"), |e| matches!(e, E::NotMonotone)),
        (fixture!("bad_version_2.mon"), |e| {
            matches!(
                e,
                E::Unsupported {
                    version: 2,
                    cell_size: 2
                }
            )
        }),
        (fixture!("bad_cell_size_1.mon"), |e| {
            matches!(
                e,
                E::Unsupported {
                    version: 1,
                    cell_size: 1
                }
            )
        }),
        (fixture!("bad_13_tracks.mon"), |e| {
            matches!(e, E::TooManyTracks(13))
        }),
        (fixture!("bad_cut_title.mon"), |e| matches!(e, E::Truncated)),
        (fixture!("bad_cut_order.mon"), |e| matches!(e, E::Truncated)),
        (fixture!("bad_cut_cells.mon"), |e| matches!(e, E::Truncated)),
        (fixture!("bad_empty.mon"), |e| matches!(e, E::Truncated)),
    ];
    for (bytes, expected) in cases {
        let error = Monotone::from_bytes(bytes).unwrap_err();
        assert!(expected(&error), "{error}");
    }
    assert!(matches!(
        Monotone::open("no such song.mon").unwrap_err(),
        E::Io(_)
    ));
}

#[test]
fn every_prefix_and_corrupted_byte_is_handled() {
    let whole = fixture!("demo.mon");
    for len in 0..whole.len() {
        assert!(matches!(
            Monotone::from_bytes(&whole[..len]),
            Err(MonotoneError::Truncated)
        ));
    }
    // Header and order bytes set to every interesting value; songs that
    // load must render.
    let mut state = 1_u32;
    for _ in 0..400 {
        let mut bytes = whole.to_vec();
        for _ in 0..4 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let at = (state >> 8) as usize % 360;
            bytes[at] = (state >> 20) as u8;
        }
        if let Ok(song) = Monotone::from_bytes(&bytes) {
            render(&song, 4096, 44100.0);
        }
    }
}

#[test]
fn any_cells_render_at_any_rate() {
    let mut state = 7_u32;
    let mut cells = Vec::new();
    for row in 0..64 {
        for track in 0..12 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            cells.push((row, track, (state >> 16) as u16));
        }
    }
    let song = Monotone::from_bytes(&song_bytes(12, &cells, &[0, 7, 0])).unwrap();
    for hardware in [1, 5, 12] {
        song.set_params(hardware, MonotoneWaveform::FSaw).unwrap();
        for rate in [1.0, 30.0, 59.9, 44100.0, 1e9, f32::INFINITY, f32::NAN, 0.0] {
            for sample in render(&song, 3000, rate) {
                assert!(sample.is_nan() || sample.abs() <= 12.0, "{sample}");
            }
        }
    }
}

#[test]
fn an_empty_song_is_silent() {
    let song = Monotone::new();
    assert_eq!(song.song().tracks(), 0);
    assert!(render(&song, 44100, 44100.0).iter().all(|&s| s == 0.0));
}

#[test]
fn params_are_checked() {
    let song = Monotone::new();
    assert_eq!(
        (song.hardware_channels(), song.waveform()),
        (1, MonotoneWaveform::Square)
    );
    song.set_params(12, MonotoneWaveform::Sin).unwrap();
    for (hardware, wave) in [(0, 0), (13, 0), (2, -1)] {
        assert!(matches!(
            song.set_params_raw(hardware, wave),
            Err(MonotoneError::InvalidParams { .. })
        ));
    }
    assert_eq!(
        (song.hardware_channels(), song.waveform_raw()),
        (12, waveform::SIN)
    );
    // Clones share the settings; values past 8 play square waves.
    let clone = song.clone();
    clone.set_params_raw(3, 99).unwrap();
    assert_eq!((song.hardware_channels(), song.waveform_raw()), (3, 99));
    assert_eq!(song.waveform(), MonotoneWaveform::Square);
}

#[test]
fn waveforms_are_soloud_values() {
    let core = [
        waveform::SQUARE,
        waveform::SAW,
        waveform::SIN,
        waveform::TRIANGLE,
        waveform::BOUNCE,
        waveform::JAWS,
        waveform::HUMPS,
        waveform::FSQUARE,
        waveform::FSAW,
    ];
    for (value, wave) in core.into_iter().zip(MonotoneWaveform::ALL) {
        assert_eq!(i32::from(wave), value);
        assert_eq!(MonotoneWaveform::from_i32(value), Some(wave));
    }
    for value in [-1, 9, i32::MAX] {
        assert_eq!(MonotoneWaveform::from_i32(value), None);
    }
}

#[test]
fn slow_rates_tick_every_sample() {
    // Below 60 Hz a tick is a sample: 4 ticks a row.
    let bytes = song_bytes(1, &[(0, 0, cell(40, 0, 0))], &[0]);
    let song = Monotone::from_bytes(&bytes).unwrap();
    let mut instance = MonotoneInstance::new(Arc::clone(&song.song), Arc::clone(&song.live));
    let mut out = [0.0; 300];
    instance.get_audio(&mut out, 300, 300, &GeneratorContext::new(false, 30.0));
    assert!(out.iter().any(|&s| s != out[0]));
}

#[test]
fn notes_past_the_table_play_its_last_entry() {
    let high = Monotone::from_bytes(&song_bytes(1, &[(0, 0, cell(126, 0, 0o77))], &[0])).unwrap();
    let top = Monotone::from_bytes(&song_bytes(1, &[(0, 0, cell(100, 0, 0))], &[0])).unwrap();
    assert_eq!(render(&high, 8192, 44100.0), render(&top, 8192, 44100.0));
}
