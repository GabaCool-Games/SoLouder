use solouder_core::GeneratorContext;

use super::envelope::Envelope;
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

const GOOD: [&[u8]; 8] = [
    fixture!("sid_6581.dump"),
    fixture!("sid_8580.zx7.dump"),
    fixture!("sid_8580db.dump"),
    fixture!("sid_6581r1.zx7.dump"),
    fixture!("ted.dump"),
    fixture!("sidted.zx7.dump"),
    fixture!("sidted_6.dump"),
    fixture!("odd_length.dump"),
];

/// `frames` of a new voice, in 512-frame reads, and the voice.
fn render(tune: &TedSid, frames: usize) -> (Vec<f32>, Box<dyn GeneratorInstance>) {
    let mut instance = tune.instance();
    let context = GeneratorContext::new(false, TED_SOUND_CLOCK as f32);
    let mut out = vec![0.0; frames];
    for block in out.chunks_mut(512) {
        let len = block.len();
        assert_eq!(instance.get_audio(block, len, 512, &context), len);
        assert!(!instance.has_ended());
    }
    (out, instance)
}

#[test]
fn fixtures_load_with_their_models() {
    let models = [
        SidModel::Mos6581,
        SidModel::Mos8580,
        SidModel::Mos8580DigiBoost,
        SidModel::Mos6581R1,
        SidModel::Mos6581,
        SidModel::Mos8580,
        SidModel::Mos6581,
        SidModel::Mos6581,
    ];
    for (bytes, model) in GOOD.iter().zip(models) {
        let tune = TedSid::from_bytes(bytes).unwrap();
        assert_eq!(tune.model(), Some(model));
        let read = TedSid::from_reader(*bytes).unwrap();
        assert_eq!(read.dump.unwrap().ops, tune.dump.as_ref().unwrap().ops);
        let (out, _) = render(&tune, 20_000);
        assert!(out.iter().all(|sample| sample.is_finite()));
        assert!(out.iter().any(|&sample| sample != out[0]), "a silent tune");
    }
    let both = TedSid::from_bytes(fixture!("sidted.zx7.dump")).unwrap();
    let dump = both.dump.unwrap();
    assert_eq!((dump.chip, dump.loop_pos), (5, 1025));
    assert!(dump.length > 1024);
    let odd = TedSid::from_bytes(fixture!("odd_length.dump")).unwrap();
    let dump = odd.dump.unwrap();
    assert_eq!(dump.length % 2, 1);
    assert_eq!(dump.ops.last().unwrap() >> 8, 0);
}

#[test]
fn damaged_fixtures_fail() {
    type Case<'a> = (&'a [u8], fn(&ChipDumpError) -> bool);
    let cases: [Case; 11] = [
        (fixture!("bad_short.dump"), |e| {
            matches!(e, ChipDumpError::Truncated)
        }),
        (fixture!("bad_magic.dump"), |e| {
            matches!(e, ChipDumpError::NotChipTune)
        }),
        (fixture!("bad_chip_1.dump"), |e| {
            matches!(e, ChipDumpError::UnsupportedChip(1))
        }),
        (fixture!("bad_no_chunks.dump"), |e| {
            matches!(e, ChipDumpError::BadHeader)
        }),
        (fixture!("bad_loop_end.dump"), |e| {
            matches!(e, ChipDumpError::BadHeader)
        }),
        (fixture!("bad_data_offset.dump"), |e| {
            matches!(e, ChipDumpError::BadHeader)
        }),
        (fixture!("bad_cut.dump"), |e| {
            matches!(e, ChipDumpError::Truncated)
        }),
        (fixture!("bad_no_wait.dump"), |e| {
            matches!(e, ChipDumpError::NoWait)
        }),
        (fixture!("bad_zero_waits.dump"), |e| {
            matches!(e, ChipDumpError::NoWait)
        }),
        (fixture!("bad_zx7_overrun.dump"), |e| {
            matches!(e, ChipDumpError::BadCompression)
        }),
        (fixture!("bad_zx7_cut.dump"), |e| {
            matches!(e, ChipDumpError::BadCompression)
        }),
    ];
    for (bytes, expected) in cases {
        let error = TedSid::from_bytes(bytes).unwrap_err();
        assert!(expected(&error), "{error:?}");
        assert!(!error.to_string().is_empty());
    }
    let missing = TedSid::open("no such dump.dump").unwrap_err();
    assert!(matches!(missing, ChipDumpError::Io(_)));
}

#[test]
fn damaged_dumps_never_panic() {
    for bytes in GOOD {
        for len in (0..bytes.len()).step_by(7) {
            if let Ok(tune) = TedSid::from_bytes(&bytes[..len]) {
                render(&tune, 600);
            }
        }
        let mut damaged = bytes.to_vec();
        let mut seed = 0x1234_5678_u32;
        for _ in 0..300 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let at = (seed >> 8) as usize % damaged.len();
            damaged[at] ^= (seed >> 24) as u8 | 1;
            if let Ok(tune) = TedSid::from_bytes(&damaged) {
                render(&tune, 600);
            }
        }
    }
}

#[test]
fn no_dump_renders_the_idle_chips() {
    let tune = TedSid::new();
    assert_eq!(tune.model(), None);
    let (out, voice) = render(&tune, 2000);
    assert!(out.iter().all(|sample| sample.is_finite()));
    assert_eq!(voice.info(24), 0.0);
}

#[test]
fn info_reports_the_registers_last_written() {
    let tune = TedSid::from_bytes(fixture!("sidted_6.dump")).unwrap();
    let (_, voice) = render(&tune, 30_000);
    let dump = tune.dump.as_ref().unwrap();
    let mut regs = [0_u8; 128];
    let mut pos = 0;
    let mut samples = 0_u64;
    let mut wait = 0_u32;
    // Replay the operations the voice ran for its 30000 samples.
    while samples < 30_000 {
        while wait == 0 {
            let op = dump.op(pos);
            pos += 2;
            if pos >= dump.length {
                pos = dump.loop_pos;
            }
            if op & 0x8000 != 0 {
                wait = u32::from(op & 0x7fff);
            } else {
                regs[usize::from(op >> 8) & 127] = op as u8;
            }
        }
        wait -= 1;
        samples += 1;
    }
    for key in 0..128 {
        assert_eq!(voice.info(key), f32::from(regs[key as usize]), "key {key}");
        assert_eq!(voice.info(key + 128), voice.info(key));
    }
    assert_ne!(voice.info(24), 0.0);
    assert!((64..69).any(|key| voice.info(key) != 0.0));
}

#[test]
fn envelope_rate_starts_in_range() {
    // Reference patch 0082: the rate is an index from the start.
    let mut envelope = Envelope::new();
    assert_eq!(envelope.run(100_000), 0);
}

#[test]
fn attack_from_full_level_wraps() {
    // Reference patch 0081: the level is 8 bits, so attacking from 0xFF
    // wraps to 0 and climbs again, as in reSID.
    let mut envelope = Envelope::new();
    envelope.set_attack_decay(0x00);
    envelope.set_sustain_release(0xFF);
    envelope.set_gate(1);
    while envelope.run(1) != 0xFF {}
    envelope.set_gate(0);
    envelope.set_gate(1);
    let mut wrapped = false;
    for _ in 0..20_000 {
        let level = envelope.run(1);
        assert!(level <= 0xFF);
        wrapped |= level == 0;
    }
    assert!(wrapped);
    assert_eq!(envelope.level(), 0xFF);
}
