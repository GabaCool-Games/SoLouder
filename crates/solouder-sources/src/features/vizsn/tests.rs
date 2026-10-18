use solouder_core::GeneratorContext;

use super::*;

fn context() -> GeneratorContext {
    GeneratorContext::new(false, SAMPLE_RATE)
}

/// `frames` samples of a voice, read `block` frames at a time; after its
/// end, zeros.
fn render(voice: &mut dyn GeneratorInstance, block: usize, frames: usize) -> Vec<f32> {
    let mut out = Vec::new();
    let mut buffer = vec![f32::NAN; block];
    while out.len() < frames {
        let got = voice.get_audio(&mut buffer, block, block, &context());
        assert!(
            got == block || voice.has_ended(),
            "short reads only at the end"
        );
        assert!(
            buffer[got..].iter().all(|&s| s == 0.0),
            "the rest is silent"
        );
        out.extend_from_slice(&buffer);
    }
    out.truncate(frames);
    out
}

#[test]
fn text_becomes_phones() {
    let vizsn = Vizsn::new(b"ma\x84\x94{| 1A\0ko");
    assert_eq!(
        &*vizsn.phones,
        &[P_CLR, 18, 0, 1, 3, 1, 3, P_CLR, P_CLR, P_CLR, P_END]
    );
    assert_eq!(&*Vizsn::new("").phones, &[P_CLR, P_END]);
    assert_eq!(&*Vizsn::default().phones, &[P_END]);
}

#[test]
fn tail_is_played_then_silence() {
    let vizsn = Vizsn::new("ma");
    assert_eq!(vizsn.samples(), 2200);
    let mut voice = vizsn.instance();
    let first = render(voice.as_mut(), 2199, 2199);
    assert!(!voice.has_ended());
    assert!(first.iter().all(|s| (0.0..=1.0).contains(s)));
    assert!(
        first.iter().any(|&s| s != 128.0 / 255.0),
        "the text is heard"
    );
    let mut rest = vec![1.0; 100];
    assert_eq!(voice.get_audio(&mut rest, 100, 100, &context()), 1);
    assert!(rest[0] != 0.0, "the last sample of the last phone");
    assert!(rest[1..].iter().all(|&s| s == 0.0));
    assert!(voice.has_ended());
}

#[test]
fn no_text_ends_at_once() {
    let mut voice = Vizsn::default().instance();
    assert!(voice.has_ended());
    assert_eq!(voice.get_audio(&mut [1.0; 8], 8, 8, &context()), 0);
    assert!(render(voice.as_mut(), 64, 64).iter().all(|&s| s == 0.0));
    assert_eq!(Vizsn::default().samples(), 0);
    assert_eq!(Vizsn::new("").samples(), 600);
}

#[test]
fn reads_of_any_size_agree() {
    let vizsn = Vizsn::new("hei sisko, mit\u{84} kuuluu");
    let expected = render(vizsn.instance().as_mut(), 512, 20_000);
    for block in [1, 7, 799, 800, 801, 4096, 20_000] {
        let got = render(vizsn.instance().as_mut(), block, 20_000);
        assert!(
            got.iter()
                .zip(&expected)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "block {block}"
        );
    }
}

#[test]
fn voices_and_rewinds_are_alike() {
    let vizsn = Vizsn::new("hei sisko");
    let mut first = vizsn.instance();
    let a = render(first.as_mut(), 1000, 9000);
    let b = render(vizsn.instance().as_mut(), 1000, 9000);
    assert_eq!(a, b, "each voice has its own noise");
    assert!(first.rewind());
    assert_eq!(render(first.as_mut(), 333, 9000), a);
}

#[test]
fn arbitrary_text_never_panics() {
    let mut seed: u32 = 1;
    let mut next = || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (seed >> 16) as u8
    };
    for len in 0..40 {
        let text: Vec<u8> = (0..len).map(|_| next()).collect();
        let vizsn = Vizsn::new(&text);
        let samples = vizsn.samples();
        let mut voice = vizsn.instance();
        let out = render(voice.as_mut(), 997, samples + 1);
        assert!(out.iter().all(|s| (0.0..=1.0).contains(s)));
        assert!(voice.has_ended(), "ends after {samples} samples");
    }
}
