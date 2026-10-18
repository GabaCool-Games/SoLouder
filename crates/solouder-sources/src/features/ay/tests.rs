use solouder_core::GeneratorContext;

use super::*;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/chipdump/",
            $name
        ))
        .as_slice()
    };
}

const GOOD: [&[u8]; 7] = [
    fixture!("ay_ay.dump"),
    fixture!("ay_ym.zx7.dump"),
    fixture!("ay_turbo.dump"),
    fixture!("ay_next.zx7.dump"),
    fixture!("ay_slow.dump"),
    fixture!("ay_fast.zx7.dump"),
    fixture!("ay_long.zx7.dump"),
];

fn context() -> GeneratorContext {
    GeneratorContext::new(false, 44100.0)
}

/// Both channels of a voice, read `block` frames at a time until it ends
/// or `limit` frames, as `[left..., right...]` per read, and the frames it
/// returned.
fn render(voice: &mut dyn GeneratorInstance, block: usize, limit: usize) -> (Vec<f32>, usize) {
    let mut out = Vec::new();
    let mut frames = 0;
    let mut buffer = vec![0.0; 2 * block];
    while frames < limit && !voice.has_ended() {
        buffer.fill(0.0);
        let got = voice.get_audio(&mut buffer, block, block, &context());
        assert!(got <= block);
        frames += got;
        out.extend_from_slice(&buffer);
    }
    (out, frames)
}

/// A stored AY dump of `ops` at `cpu` ticks a second.
fn dump(ops: &[u16], cpu: u32) -> Vec<u8> {
    let mut out = b"CHIPTUNE".to_vec();
    let length = ops.len() * 2;
    for word in [28, 0x101, 1, length, 0, 0] {
        out.extend_from_slice(&(word as u16).to_le_bytes());
    }
    out.extend_from_slice(&cpu.to_le_bytes());
    out.extend_from_slice(&1_774_400_u32.to_le_bytes());
    out.extend(ops.iter().flat_map(|op| op.to_le_bytes()));
    out
}

#[test]
fn fixtures_load_and_play() {
    let expected = [
        (1, false, 50),
        (1, true, 50),
        (2, true, 50),
        (3, false, 60),
        (1, false, 11),
        (1, true, 1000),
        (1, false, 50),
    ];
    for (bytes, (chip, ym, cpu)) in GOOD.iter().zip(expected) {
        let ay = Ay::from_bytes(bytes).unwrap();
        let chip_type = ay.chip_type().map(AyChipType::to_byte);
        assert_eq!((chip_type, ay.is_ym()), (Some(chip), Some(ym)));
        assert_eq!(ay.speeds().unwrap().0, cpu);
        let read = Ay::from_reader(*bytes).unwrap();
        assert_eq!(read.dump.unwrap().ops, ay.dump.as_ref().unwrap().ops);
        let mut voice = ay.instance();
        let (out, frames) = render(voice.as_mut(), 512, 2_000_000);
        assert!(voice.has_ended(), "dumps end");
        assert!(frames > 1000);
        assert!(
            out.iter()
                .all(|&sample| sample.is_finite() && sample >= 0.0)
        );
        assert!(out.iter().any(|&sample| sample != out[0]), "a silent dump");
    }
}

#[test]
fn damaged_fixtures_fail() {
    type Case<'a> = (&'a [u8], fn(&ChipDumpError) -> bool);
    let cases: [Case; 7] = [
        (fixture!("bad_ay_chip_0.dump"), |e| {
            matches!(e, ChipDumpError::UnsupportedChip(0))
        }),
        (fixture!("bad_ay_cpu_0.dump"), |e| {
            matches!(e, ChipDumpError::UnsupportedSpeed)
        }),
        (fixture!("bad_ay_cpu_10.dump"), |e| {
            matches!(e, ChipDumpError::UnsupportedSpeed)
        }),
        (fixture!("bad_ay_chip_speed.dump"), |e| {
            matches!(e, ChipDumpError::UnsupportedSpeed)
        }),
        (fixture!("bad_ay_no_chunks.dump"), |e| {
            matches!(e, ChipDumpError::BadHeader)
        }),
        (fixture!("bad_ay_cut.dump"), |e| {
            matches!(e, ChipDumpError::Truncated)
        }),
        (fixture!("bad_ay_zx7_cut.dump"), |e| {
            matches!(e, ChipDumpError::BadCompression)
        }),
    ];
    for (bytes, expected) in cases {
        let error = Ay::from_bytes(bytes).unwrap_err();
        assert!(expected(&error), "{error:?}");
        assert!(!error.to_string().is_empty());
    }
    assert!(Ay::from_bytes(fixture!("sid_6581.dump")).is_err());
    // Ay does not use the loop position, so one at the end is kept.
    let long = Ay::from_bytes(fixture!("ay_long.zx7.dump")).unwrap();
    let dump = long.dump.unwrap();
    assert_eq!(dump.loop_pos, dump.length);
    assert!(matches!(
        Ay::open("no such dump.dump"),
        Err(ChipDumpError::Io(_))
    ));
}

#[test]
fn damaged_dumps_never_panic() {
    for bytes in GOOD {
        for len in (0..bytes.len()).step_by(11) {
            if let Ok(ay) = Ay::from_bytes(&bytes[..len]) {
                render(ay.instance().as_mut(), 700, 30_000);
            }
        }
        let mut damaged = bytes.to_vec();
        let mut seed = 0x8765_4321_u32;
        for _ in 0..200 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let at = (seed >> 8) as usize % damaged.len();
            damaged[at] ^= (seed >> 24) as u8 | 1;
            if let Ok(ay) = Ay::from_bytes(&damaged) {
                render(ay.instance().as_mut(), 700, 30_000);
            }
        }
    }
}

#[test]
fn no_dump_ends_at_once() {
    let ay = Ay::new();
    assert_eq!(
        (ay.chip_type(), ay.is_ym(), ay.speeds()),
        (None, None, None)
    );
    let mut voice = ay.instance();
    assert!(voice.has_ended());
    let mut out = [1.0; 64];
    assert_eq!(voice.get_audio(&mut out, 32, 32, &context()), 0);
    assert_eq!(voice.info(8), 0.0);
}

#[test]
fn short_reads_put_the_right_channel_at_the_stride() {
    // Reference patch 0091.
    let ay = Ay::from_bytes(fixture!("ay_ay.dump")).unwrap();
    let (mut full, mut part) = (ay.instance(), ay.instance());
    let mut a = vec![-1.0; 1024];
    let mut b = vec![-1.0; 1024];
    full.get_audio(&mut a, 512, 512, &context());
    part.get_audio(&mut b, 100, 512, &context());
    assert_eq!(b[..100], a[..100]);
    assert_eq!(b[512..612], a[512..612]);
    assert!(b[100..512].iter().chain(&b[612..]).all(|&x| x == -1.0));
}

#[test]
fn the_last_write_long_waits_and_the_tail_play() {
    // Reference patches 0096, 0097 and 0098: a tone for one wait of 60
    // ticks at 50 Hz (52920 frames), then a last write.
    let ops = [0x073e, 0x0040, 0x080f, 0x8000 | 60, 0x0803];
    let ay = Ay::from_bytes(&dump(&ops, 50)).unwrap();
    let mut voice = ay.instance();
    let (out, frames) = render(voice.as_mut(), 512, 200_000);
    assert!((52_900..=53_000).contains(&frames), "{frames}");
    assert_eq!(voice.info(8), 3.0);
    let middle = &out[99 * 1024..99 * 1024 + 512];
    let high = middle.iter().fold(f32::MIN, |m, &x| m.max(x));
    let low = middle.iter().fold(f32::MAX, |m, &x| m.min(x));
    assert!(high - low > 0.1, "the tone plays through the wait");
}

#[test]
fn rewind_restarts_the_chips() {
    // Reference patch 0099.
    let ay = Ay::from_bytes(fixture!("ay_turbo.dump")).unwrap();
    let mut voice = ay.instance();
    render(voice.as_mut(), 512, 1536);
    assert!(voice.rewind());
    let (again, _) = render(voice.as_mut(), 512, 2048);
    let (fresh, _) = render(ay.instance().as_mut(), 512, 2048);
    assert_eq!(again, fresh);
}

#[test]
fn info_reports_both_chips() {
    // Mixer 0x07 on the first chip, then 0x3C on the second (0x17).
    let ops = [0x0707, 0x173c, 0x0812, 0x8001];
    let ay = Ay::from_bytes(&dump(&ops, 50)).unwrap();
    let mut voice = ay.instance();
    render(voice.as_mut(), 512, 512);
    assert_eq!(
        (voice.info(7), voice.info(0x17), voice.info(0x37)),
        (7.0, 60.0, 60.0)
    );
    // Register 8 keeps 5 bits.
    assert_eq!(voice.info(8), 18.0);
    for key in [14, 15, 0x1E, 0x1F, 0x27, 0x2E] {
        assert_eq!(voice.info(key), 0.0, "key {key}");
    }
}

/// A stored dump of `ops` in as many chunks as they take, at tick rate
/// `cpu` and chip clock `chip`.
fn long_dump(ops: &[u16], cpu: u32, chip: u32) -> Vec<u8> {
    let length = ops.len() * 2;
    let chunks = length.div_ceil(1024);
    let mut out = b"CHIPTUNE".to_vec();
    for word in [28, 0x101, chunks, length - (chunks - 1) * 1024, 0, 0] {
        out.extend_from_slice(&(word as u16).to_le_bytes());
    }
    out.extend_from_slice(&cpu.to_le_bytes());
    out.extend_from_slice(&chip.to_le_bytes());
    out.extend(ops.iter().flat_map(|op| op.to_le_bytes()));
    out
}

#[test]
fn chip_clocks_past_4_mhz_fail() {
    // Reference patch 0113.
    let tune = [0x073e, 0x080f, 0x8000 | 4];
    for chip in [4_000_001, u32::MAX] {
        let error = Ay::from_bytes(&long_dump(&tune, 50, chip)).unwrap_err();
        assert!(
            matches!(error, ChipDumpError::UnsupportedSpeed),
            "{error:?}"
        );
    }
    let ay = Ay::from_bytes(&long_dump(&tune, 50, 4_000_000)).unwrap();
    assert!(render(ay.instance().as_mut(), 512, 100_000).1 > 0);
}

#[test]
fn busy_dumps_fail() {
    // Reference patch 0114: at most 131072 operations before waits of two
    // frames' ticks, 8 at 50 Hz, counting restarts.
    let load = |ops: &[u16]| Ay::from_bytes(&long_dump(ops, 50, 1_774_400));
    let busy = |ops: &[u16]| matches!(load(ops), Err(ChipDumpError::TooManyOperations));
    let mut ops = vec![0x080f; 131_071];
    ops.push(0x8000 | 8);
    let ay = load(&ops).unwrap();
    assert!(render(ay.instance().as_mut(), 512, 4096).1 > 0);
    ops.insert(0, 0x090f);
    assert!(busy(&ops));
    assert!(busy(&[0x080f; 131_073]), "without waits too");
    // 16383 writes and a wait of 1 play 8 times in two frames.
    let mut ops = vec![0x080f; 16_383];
    ops.push(0x8001);
    assert!(load(&ops).is_ok());
    ops.insert(0, 0x090f);
    assert!(busy(&ops));
    assert!(
        load(&[0x080f; 3]).is_ok(),
        "a dump that never waits plays nothing again"
    );
}
