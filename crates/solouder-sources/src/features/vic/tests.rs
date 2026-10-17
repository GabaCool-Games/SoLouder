use super::*;

fn render(instance: &mut dyn GeneratorInstance, frames: usize) -> Vec<f32> {
    let mut out = vec![f32::NAN; frames];
    let context = GeneratorContext::new(false, 44100.0);
    assert_eq!(
        instance.get_audio(&mut out, frames, frames, &context),
        frames
    );
    out
}

#[test]
fn the_noise_table_follows_the_lfsr() {
    // 0xACE1 >> 1 = 0x5670 (the low bit was 1) ^ 0xB400 = 0xE270.
    assert_eq!(NOISE[0], 0x70 ^ 0xE2);
    let distinct = NOISE
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    assert!(distinct > 200);
}

#[test]
fn registers_past_the_last_are_rejected() {
    let vic = Vic::new();
    assert_eq!(vic.register(Vic::NOISE), Some(0));
    assert_eq!(vic.register(VIC_REGISTERS), None);
    assert_eq!(vic.set_register(4, 1), Err(RegisterOutOfRange(4)));
    vic.set_register(Vic::ALTO, 200).unwrap();
    assert_eq!(vic.register(Vic::ALTO), Some(200));
    assert_eq!(vic.model(), VicModel::Pal);
    vic.set_model(VicModel::Ntsc);
    assert_eq!(vic.model(), VicModel::Ntsc);
    assert_eq!(VicModel::from_index(2), None);
}

#[test]
fn oscillators_below_128_are_silent() {
    let vic = Vic::new();
    for reg in 0..VIC_REGISTERS {
        vic.set_register(reg, 127).unwrap();
    }
    let mut instance = vic.instance();
    assert!(render(instance.as_mut(), 600).iter().all(|&s| s == 0.0));
    assert!(!instance.has_ended());
}

#[test]
fn squares_toggle_at_their_clock_and_registers_apply_per_block() {
    let vic = Vic::new();
    vic.set_register(Vic::BASS, 254).unwrap();
    let mut instance = vic.instance();
    let out = render(instance.as_mut(), 64);
    // Step (4329 * 65536 / 44100 + 0.5) = 6433: high for 6 frames.
    let step = (4329.0_f32 * 65536.0 / 44100.0 + 0.5) as u32;
    assert_eq!(step, 6433);
    assert_eq!(&out[..6], &[0.125; 6]);
    assert_eq!(out[6], -0.125);
    // A register written between reads applies from the next one.
    vic.set_register(Vic::BASS, 0).unwrap();
    assert!(render(instance.as_mut(), 32).iter().all(|&s| s == 0.0));
    vic.set_register(Vic::NOISE, 255).unwrap();
    let noisy = render(instance.as_mut(), 32);
    let first = f32::from(NOISE[0]) / 255.0 - 0.5;
    assert_eq!(noisy[0], first / 4.0);
    assert_ne!(noisy[0], noisy[1]);
}
