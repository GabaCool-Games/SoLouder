use solouder_core::GeneratorContext;

use super::*;

fn context() -> GeneratorContext {
    GeneratorContext::new(false, 11025.0)
}

/// A voice's samples, read `block` frames at a time until it ends or
/// `limit` frames.
fn render(voice: &mut dyn GeneratorInstance, block: usize, limit: usize) -> Vec<f32> {
    let mut out = Vec::new();
    let mut buffer = vec![0.0; block];
    while out.len() < limit && !voice.has_ended() {
        buffer.fill(0.0);
        let got = voice.get_audio(&mut buffer, block, block, &context());
        assert!(got <= block);
        assert!(
            got == block || voice.has_ended(),
            "short reads only at the end"
        );
        out.extend_from_slice(&buffer[..got]);
    }
    out
}

#[test]
fn tables_read_as_compiled() {
    let table = elements::elements();
    assert_eq!(table.len(), 69);
    assert_eq!((table[0].name, table[0].rank), ("END", 31));
    assert_eq!(table[68].name, "OR");
    assert_eq!(table.iter().filter(|e| e.vowel).count(), 23);
    assert_eq!(elements::longest(), 16);
    // ER: 2, 16, 16, its fifth interpolator 0..
    let er = table.iter().find(|e| e.name == "ER").unwrap();
    assert_eq!((er.rank, er.du, er.ud, er.vowel), (2, 16, 16, true));
    let an = table[0].interp[7];
    assert_eq!((an.steady, an.fixed, an.proportion), (-30.0, -10.5, 100));
    let groups = rules::groups();
    assert_eq!(groups.len(), 27);
    assert_eq!(groups.iter().map(Vec::len).sum::<usize>(), 388);
    for group in groups {
        assert!(
            group.last().unwrap().matched.is_none(),
            "each group ends in its catch-all"
        );
        assert_eq!(group.iter().filter(|r| r.matched.is_none()).count(), 1);
    }
    assert_eq!(groups[1][0].right, " ");
}

fn phonemes(text: &str) -> String {
    let mut out = Vec::new();
    tts::xlate_string(text.as_bytes(), &mut out);
    String::from_utf8(out).unwrap()
}

#[test]
fn text_becomes_phonemes() {
    // The rules end a word with the space after it, then xlate_word adds one.
    assert_eq!(phonemes("hello"), "hel@U  ");
    // Numbers are read, capitals spelled, [phonemes] copied.
    assert!(phonemes("42").starts_with(&phonemes("forty")));
    assert_eq!(phonemes("NASA"), phonemes("en ay es ay"));
    assert_eq!(phonemes("[h@'l@Ut]"), "h@'l@Ut ");
    assert_eq!(
        phonemes("Hi! Ok."),
        format!("{}.{}.", phonemes("Hi"), phonemes("Ok"))
    );
    // Bytes from 0x80 are spelled as their low 7 bits: 0xC3 is "see".
    let mut out = Vec::new();
    tts::xlate_string(b"\xc3", &mut out);
    assert_eq!(out, phonemes("see").as_bytes());
    // Numbers past i32 wrap, as in the reference's build.
    let mut out = Vec::new();
    tts::xlate_string(b"4294967297", &mut out);
    assert_eq!(out, phonemes("one").as_bytes());
}

#[test]
fn stress_marks_apply() {
    // '@' (ELM_A, 52) with primary stress, then a byte that is no phoneme
    // (reference patch 0104).
    let mut out = Vec::new();
    elements::phone_to_elm(b"'@?", &mut out);
    assert_eq!(out, [52, 4, 3]);
    // The last phoneme is looked up alone (reference patch 0103).
    let mut out = Vec::new();
    elements::phone_to_elm(b"@", &mut out);
    assert_eq!(out.len(), 3);
}

#[test]
fn speech_plays_and_ends() {
    let speech = Speech::new("Hello world, this is SoLouder.");
    assert_eq!(speech.channels(), 1);
    assert_eq!(speech.sample_rate(), 11025.0);
    assert!(speech.frames() > 0);
    let mut voice = speech.instance();
    let audio = render(voice.as_mut(), 512, 1 << 20);
    assert!(voice.has_ended());
    assert!(audio.len() > 11025 && audio.len() < 1 << 20);
    assert!(audio.iter().any(|&s| s.abs() > 0.01));
    assert!(audio.iter().all(|s| s.abs() <= 1.0));
    // Voices are deterministic.
    let mut again = speech.instance();
    assert_eq!(render(again.as_mut(), 512, 1 << 20), audio);
}

#[test]
fn no_text_ends_at_once() {
    for speech in [Speech::default(), Speech::new(""), Speech::new("\0hello")] {
        let mut voice = speech.instance();
        let mut out = [0.0; 64];
        assert_eq!(voice.get_audio(&mut out, 64, 64, &context()), 0);
        assert!(voice.has_ended());
    }
}

#[test]
fn speeds_are_checked() {
    let speech = Speech::new("hello");
    for speed in [0.0, -10.0, f32::NAN, 0.05, 1001.0, f32::INFINITY] {
        let params = SpeechParams {
            speed,
            ..SpeechParams::default()
        };
        assert!(speech.set_params(params).is_err(), "{speed}");
    }
    assert_eq!(speech.params(), SpeechParams::default());
    for speed in [0.1, 1000.0] {
        let params = SpeechParams {
            speed,
            waveform: KlattWaveform::Saw,
            ..SpeechParams::default()
        };
        assert_eq!(speech.set_params(params), Ok(()));
        assert_eq!(speech.params(), params);
    }
}

#[test]
fn voices_keep_their_speed() {
    let speech = Speech::new("hello world");
    let plain = Speech::new("hello world");
    let mut voice = speech.instance();
    let mut changed = render(voice.as_mut(), 512, 1024);
    let slow = SpeechParams {
        speed: 80.0,
        ..SpeechParams::default()
    };
    speech.set_params(slow).unwrap();
    changed.extend(render(voice.as_mut(), 512, 3072));
    assert_eq!(changed, render(plain.instance().as_mut(), 512, 4096));
    // A rewound voice keeps its speed too; rewind keeps the filters'
    // history, so compare with a voice of plain rewound at the same point.
    let mut plain_voice = plain.instance();
    render(plain_voice.as_mut(), 512, 4096);
    assert!(voice.rewind() && plain_voice.rewind());
    assert_eq!(
        render(voice.as_mut(), 512, 2048),
        render(plain_voice.as_mut(), 512, 2048)
    );
    // New voices take the new speed.
    let slow_audio = render(speech.instance().as_mut(), 512, 1 << 22);
    let plain_audio = render(plain.instance().as_mut(), 512, 1 << 22);
    assert!(slow_audio.len() > 7 * plain_audio.len());
}

#[test]
fn every_waveform_and_frequency_plays() {
    for index in 0..7 {
        let waveform = KlattWaveform::from_index(index).unwrap();
        assert_eq!(waveform.index(), index);
        for frequency in [0, 1, 1330, 100_000, u32::MAX] {
            let speech = Speech::new("one two");
            let params = SpeechParams {
                frequency,
                declination: 3.0,
                waveform,
                ..SpeechParams::default()
            };
            speech.set_params(params).unwrap();
            let audio = render(speech.instance().as_mut(), 300, 1 << 20);
            assert!(audio.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        }
    }
    assert_eq!(KlattWaveform::from_index(7), None);
}

#[test]
fn arbitrary_text_never_panics() {
    let mut seed: u32 = 1;
    let mut next = || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (seed >> 16) as u8
    };
    let alphabet =
        b"aeiouAEIOUbcdrstxyzBCDXYZ0123456789 .,;:!?'-+[]\"()#%^&@\t\n\x0b\x00\x7f\x80\xc3\xff";
    for _ in 0..400 {
        let len = usize::from(next() % 48);
        let text: Vec<u8> = (0..len)
            .map(|_| {
                let pick = next();
                if pick < 200 {
                    alphabet[usize::from(pick) % alphabet.len()]
                } else {
                    pick
                }
            })
            .collect();
        let speech = Speech::new(&text);
        let fast = SpeechParams {
            speed: 0.1,
            ..SpeechParams::default()
        };
        speech.set_params(fast).unwrap();
        let mut voice = speech.instance();
        render(voice.as_mut(), 128, 1 << 22);
        assert!(voice.has_ended(), "{text:?} ends");
    }
}
