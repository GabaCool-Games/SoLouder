use super::*;
use crate::dsp::waveform;

fn sine(frames: usize, freq: f32, rate: f32, amp: f32) -> Vec<f32> {
    (0..frames)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin() * amp)
        .collect()
}

/// Planar `channels` copies of `signal`, filtered in 512-frame calls.
fn run(instance: &mut dyn FilterInstance, signal: &[f32], channels: usize, rate: f32) -> Vec<f32> {
    let frames = signal.len();
    let mut buffer: Vec<f32> = (0..channels).flat_map(|_| signal.iter().copied()).collect();
    let mut start = 0;
    while start < frames {
        let count = (frames - start).min(512);
        let mut block = vec![0.0; count * channels];
        for c in 0..channels {
            block[c * count..][..count].copy_from_slice(&buffer[c * frames + start..][..count]);
        }
        let time = start as f64 / f64::from(rate);
        instance.filter(&mut block, count, count, channels, rate, time);
        for c in 0..channels {
            buffer[c * frames + start..][..count].copy_from_slice(&block[c * count..][..count]);
        }
        start += count;
    }
    buffer
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[test]
fn dry_fft_filters_leave_audio_unchanged() {
    let input = sine(3000, 440.0, 44100.0, 0.5);
    for filter in [&FftFilter as &dyn Filter, &BassboostFilter::default()] {
        let mut instance = filter.create_instance();
        instance.params_mut().set(0, 0.0);
        assert_eq!(
            run(instance.as_mut(), &input, 2, 44100.0),
            [&input[..], &input[..]].concat()
        );
    }
}

#[test]
fn bassboost_raises_low_frequencies_only() {
    let level = |freq: f32, boost: f32| {
        let mut bass = BassboostFilter::default();
        bass.set_params(boost).unwrap();
        let output = run(
            bass.create_instance().as_mut(),
            &sine(8192, freq, 44100.0, 0.25),
            1,
            44100.0,
        );
        rms(&output[2048..])
    };
    assert!(level(100.0, 4.0) > 2.5 * level(100.0, 1.0));
    let (high, high_boosted) = (level(5000.0, 1.0), level(5000.0, 4.0));
    assert!(
        (high_boosted / high - 1.0).abs() < 0.05,
        "{high} vs {high_boosted}"
    );
    assert!((high / rms(&sine(8192, 5000.0, 44100.0, 0.25)) - 1.0).abs() < 0.05);
}

#[test]
fn fft_filter_output_is_delayed_and_bounded() {
    let output = run(
        FftFilter.create_instance().as_mut(),
        &sine(4096, 880.0, 44100.0, 0.5),
        3,
        44100.0,
    );
    assert!(output.iter().all(|s| s.is_finite() && s.abs() < 2.0));
    assert!(output[..128].iter().all(|&s| s == 0.0));
    assert!(rms(&output[1024..4096]) > 0.01);
}

#[test]
fn waveforms_match_their_definitions() {
    assert_eq!(waveform::generate(waveform::SQUARE, 0.25), -0.5);
    assert_eq!(waveform::generate(waveform::SQUARE, 0.75), 0.5);
    assert_eq!(waveform::generate(99, 0.75), 0.5);
    assert_eq!(waveform::generate(waveform::SAW, 0.25), -0.25);
    assert_eq!(waveform::generate(waveform::SIN, 0.25), 0.5);
    assert_eq!(waveform::generate(waveform::TRIANGLE, 0.5), 0.5);
    assert_eq!(waveform::generate(waveform::TRIANGLE, 0.0), -0.5);
    assert_eq!(waveform::generate(waveform::JAWS, 0.5), -0.5);
    assert_eq!(waveform::generate(waveform::HUMPS, 0.25), 0.0);
    assert_eq!(waveform::generate(waveform::BOUNCE, 0.75), 0.0);
    assert!((waveform::generate(waveform::FSQUARE, 0.25) - 0.5).abs() < 0.05);
    assert!((waveform::generate(waveform::FSAW, 0.25) - 0.25).abs() < 0.05);
}

#[test]
fn robotize_square_gates_the_signal() {
    let input = sine(2000, 300.0, 44100.0, 0.5);
    let output = run(
        RobotizeFilter::default().create_instance().as_mut(),
        &input,
        1,
        44100.0,
    );
    for (o, i) in output.iter().zip(&input) {
        assert!(*o == 0.0 || o == i);
    }
    assert!(output.contains(&0.0) && output.iter().any(|&o| o != 0.0));
}

#[test]
fn robotize_periods_below_one_are_one() {
    let input = sine(1000, 300.0, 44100.0, 0.5);
    for freq in [0.0, -5.0, 1e9, f32::NAN, 1e-30] {
        let mut robot = RobotizeFilter::default();
        robot.set_params(freq, waveform::SAW);
        let output = run(robot.create_instance().as_mut(), &input, 2, 44100.0);
        assert!(output.iter().all(|s| s.is_finite()), "freq {freq}");
    }
}

#[test]
fn flanger_channels_share_the_lfo() {
    let input = sine(3000, 500.0, 44100.0, 0.5);
    let output = run(
        FlangerFilter::default().create_instance().as_mut(),
        &input,
        2,
        44100.0,
    );
    assert_eq!(output[..3000], output[3000..]);
    assert_ne!(output[..3000], input[..]);
}

#[test]
fn flanger_delay_extremes_are_safe() {
    let input = sine(1500, 500.0, 44100.0, 0.5);
    for delay in [0.0, -1.0, 1e12, f32::NAN, 0.02] {
        let mut instance = FlangerFilter::default().create_instance();
        instance.params_mut().set(FlangerFilter::DELAY, delay);
        let output = run(instance.as_mut(), &input, 2, 44100.0);
        assert!(output.iter().all(|s| s.is_finite()), "delay {delay}");
    }
}

#[test]
fn freeverb_leaves_mono_unchanged_and_applies_its_settings() {
    let input = sine(4000, 700.0, 44100.0, 0.5);
    let mono = run(
        FreeverbFilter::default().create_instance().as_mut(),
        &input,
        1,
        44100.0,
    );
    assert_eq!(mono, input);
    let stereo = |room: f32| {
        let mut verb = FreeverbFilter::default();
        verb.set_params(0.0, room, 0.5, 1.0).unwrap();
        run(verb.create_instance().as_mut(), &input, 2, 44100.0)
    };
    let (small, large) = (stereo(0.1), stereo(0.9));
    assert_ne!(small, large);
    assert!(rms(&large[3000..4000]) > rms(&small[3000..4000]));
}

#[test]
fn frozen_freeverb_ignores_its_input() {
    let tail = |second: &[f32]| {
        let mut instance = FreeverbFilter::default().create_instance();
        let mut first = sine(2048, 700.0, 44100.0, 0.5);
        first.extend_from_slice(&first.clone());
        instance.filter(&mut first, 2048, 2048, 2, 44100.0, 0.0);
        instance.params_mut().set(FreeverbFilter::FREEZE, 1.0);
        let mut block = second.to_vec();
        instance.filter(&mut block, 1024, 1024, 2, 44100.0, 0.1);
        block
    };
    let a = tail(&sine(2048, 300.0, 44100.0, 0.5));
    let b = tail(&vec![0.0; 2048]);
    assert_eq!(a, b);
    assert!(rms(&a) > 0.001);
}

#[test]
fn spectral_and_modulation_settings_are_validated() {
    let mut bass = BassboostFilter::default();
    assert!(bass.set_params(-0.1).is_err() && bass.set_params(f32::NAN).is_err());
    assert!(bass.set_params(0.0).is_ok());
    let mut flanger = FlangerFilter::default();
    assert!(flanger.set_params(0.0, 1.0).is_err() && flanger.set_params(0.01, f32::NAN).is_err());
    assert!(flanger.set_params(0.01, 1.0).is_ok());
    let mut verb = FreeverbFilter::default();
    for bad in [
        (1.5, 0.5, 0.5, 1.0),
        (f32::NAN, 0.5, 0.5, 1.0),
        (0.0, 0.0, 0.5, 1.0),
        (0.0, 0.5, -1.0, 1.0),
        (0.0, 0.5, 0.5, f32::NAN),
    ] {
        assert!(
            verb.set_params(bad.0, bad.1, bad.2, bad.3).is_err(),
            "{bad:?}"
        );
    }
    assert_eq!(verb, FreeverbFilter::default());
}

#[test]
fn spectral_and_modulation_parameters_are_described() {
    assert_eq!(BassboostFilter::default().param_name(1), Some("Boost"));
    assert_eq!(BassboostFilter::default().param_max(1), 10.0);
    let robot = RobotizeFilter::default();
    assert_eq!(robot.param_name(3), None);
    assert_eq!(robot.param_type(RobotizeFilter::WAVE), ParamType::Int);
    assert_eq!(robot.param_min(RobotizeFilter::FREQ), 0.1);
    let flanger = FlangerFilter::default();
    assert_eq!(flanger.param_min(FlangerFilter::FREQ), 0.001);
    let verb = FreeverbFilter::default();
    assert_eq!(verb.param_type(FreeverbFilter::FREEZE), ParamType::Bool);
    assert_eq!(verb.param_name(7), Some("Wet"));
}

#[test]
fn c_int_follows_x86() {
    use crate::dsp::c_int;
    assert_eq!(c_int(-2.9), -2);
    assert_eq!(c_int(2_147_483_647.9), i32::MAX);
    assert_eq!(c_int(-2_147_483_648.5), i32::MIN);
    assert_eq!(c_int(2_147_483_648.0), i32::MIN);
    assert_eq!(c_int(f64::NAN), i32::MIN);
}
