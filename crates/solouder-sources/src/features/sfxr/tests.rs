use solouder_core::GeneratorContext;

use super::*;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/sfxr/",
            $name
        ))
        .as_slice()
    };
}

/// Renders until the voice ends (at most `limit` frames).
fn render(sfxr: &Sfxr, looping: bool, limit: usize) -> Vec<f32> {
    let mut instance = sfxr.instance();
    let context = GeneratorContext::new(looping, 44100.0);
    let mut out = Vec::new();
    let mut block = [0.0; 512];
    while out.len() < limit {
        let got = instance.get_audio(&mut block, 512, 512, &context);
        out.extend_from_slice(&block[..got]);
        if got < 512 {
            assert!(instance.has_ended());
            break;
        }
    }
    out
}

#[test]
fn files_load_by_version() {
    let sfxr = Sfxr::from_bytes(fixture!("v102_laser.sfx")).unwrap();
    let p = sfxr.params();
    assert_eq!((p.wave_type, p.sound_vol, p.base_freq), (0, 0.4, 0.62));
    assert_eq!((p.arp_speed, p.arp_mod, p.freq_dramp), (0.6, 0.35, 0.05));
    assert!(!p.filter_on);
    // Version 100 has no `freq_dramp` or arpeggio: they keep their values.
    sfxr.load_params(fixture!("v100_sine.sfx")).unwrap();
    let p = sfxr.params();
    assert_eq!((p.wave_type, p.sound_vol, p.base_freq), (2, 0.5, 0.45));
    assert_eq!((p.arp_speed, p.arp_mod, p.freq_dramp), (0.6, 0.35, 0.05));
    let burst = Sfxr::from_bytes(fixture!("v101_trailing.sfx")).unwrap();
    assert_eq!(burst.params().sound_vol, 0.5);
    let flagged = Sfxr::from_bytes(fixture!("v102_filter_byte_2.sfx")).unwrap();
    assert!(flagged.params().filter_on);
}

#[test]
fn damaged_files_fail_without_changing_anything() {
    let sfxr = Sfxr::from_bytes(fixture!("v101_burst.sfx")).unwrap();
    let before = sfxr.params();
    for (bytes, version) in [
        (fixture!("bad_cut_102.sfx"), None),
        (fixture!("bad_cut_header.sfx"), None),
        (fixture!("bad_version_103.sfx"), Some(103)),
        (fixture!("bad_empty.sfx"), Some(0)),
        (&[100, 0][..], None),
    ] {
        match (sfxr.load_params(bytes), version) {
            (Err(SfxrError::Truncated), None) => {}
            (Err(SfxrError::UnknownVersion(v)), Some(expected)) => assert_eq!(v, expected),
            (other, _) => panic!("unexpected {other:?}"),
        }
        assert_eq!(sfxr.params(), before);
    }
    assert!(matches!(
        sfxr.load_params_file("no such file.sfx"),
        Err(SfxrError::Io(_))
    ));
}

#[test]
fn every_prefix_and_corruption_of_a_file_is_a_typed_result() {
    let whole = fixture!("v102_laser.sfx");
    for len in 0..=whole.len() {
        let result = SfxrParams::default().with_file(&whole[..len]);
        assert_eq!(result.is_ok(), len == whole.len(), "prefix of {len} bytes");
    }
    // Every byte flipped: versions fail, other fields load as whatever the
    // bytes spell (NaN and infinity included), and rendering stays bounded.
    let mut seed = 0x1234_5678_u32;
    for i in 0..whole.len() {
        let mut bytes = whole.to_vec();
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        bytes[i] ^= (seed >> 24) as u8 | 1;
        if let Ok(sfxr) = Sfxr::from_bytes(&bytes) {
            let out = render(&sfxr, false, 20_000);
            assert!(out.iter().all(|s| s.is_nan() || (-1.0..=1.0).contains(s)));
        }
    }
}

#[test]
fn extreme_settings_render_without_panicking() {
    let sfxr = Sfxr::new();
    for value in [
        f32::NAN,
        f32::INFINITY,
        -f32::INFINITY,
        1e30,
        -1e30,
        0.0,
        2.0,
    ] {
        for wave_type in [0, 1, 2, 3, 9, -1] {
            sfxr.set_params(SfxrParams {
                wave_type,
                base_freq: value,
                freq_limit: value,
                pha_offset: value,
                arp_speed: value,
                repeat_speed: value,
                env_sustain: 0.01,
                ..SfxrParams::default()
            });
            render(&sfxr, false, 4096);
            render(&sfxr, true, 4096);
        }
    }
    // A looping sound restarts through `reset`, which kept a period of 0
    // past a base frequency of about 10 (reference patch 0111).
    for base_freq in [20.0, -20.0, 11.0, 1e30] {
        sfxr.set_params(SfxrParams {
            base_freq,
            env_attack: 0.0,
            env_sustain: 0.01,
            env_decay: 0.01,
            ..SfxrParams::default()
        });
        let out = render(&sfxr, true, 8192);
        assert_eq!(out.len(), 8192, "the looping sound plays on");
        assert!(out.iter().all(|x| x.is_finite()));
    }
}

#[test]
fn presets_are_deterministic_by_seed() {
    for index in 0..7 {
        let preset = SfxrPreset::from_index(index).unwrap();
        let a = Sfxr::from_preset(preset, 42);
        let b = Sfxr::from_preset(preset, 42);
        assert_eq!(a.params(), b.params());
        assert_eq!(render(&a, false, 200_000), render(&b, false, 200_000));
    }
    assert_eq!(SfxrPreset::from_index(7), None);
    let coins: Vec<_> = (0..4)
        .map(|seed| Sfxr::from_preset(SfxrPreset::Coin, seed).params())
        .collect();
    assert!(coins.windows(2).all(|w| w[0] != w[1]));
    // Seeds whose state arithmetic overflows `int` wrap, as C does.
    Sfxr::from_preset(SfxrPreset::Explosion, i32::MAX);
}

#[test]
fn sounds_end_unless_looping() {
    let sfxr = Sfxr::from_preset(SfxrPreset::Jump, 7);
    let once = render(&sfxr, false, 1_000_000);
    assert!(!once.is_empty() && once.len() < 1_000_000);
    let mut ended = sfxr.instance();
    let context = GeneratorContext::new(false, 44100.0);
    let mut block = [0.0; 512];
    while ended.get_audio(&mut block, 512, 512, &context) == 512 {}
    assert_eq!(ended.get_audio(&mut block, 512, 512, &context), 0);
    assert!(once.iter().any(|&s| s != 0.0));
    // Looping restarts inside the source, so reads never come up short.
    let looped = render(&sfxr, true, once.len() * 3);
    assert!(looped.len() >= once.len() * 3);
    assert_eq!(&looped[..once.len()], &once[..]);
}
