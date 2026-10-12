use super::*;

fn noise(len: usize) -> Vec<f32> {
    let mut x: u32 = 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as f32 / 8_388_608.0 - 1.0
        })
        .collect()
}

fn run(filter: &dyn Filter, input: &[f32], channels: usize, rate: f32) -> Vec<f32> {
    let mut instance = filter.create_instance();
    let mut buffer = input.to_vec();
    let frames = input.len() / channels;
    instance.filter(&mut buffer, frames, frames, channels, rate, 0.0);
    buffer
}

#[test]
fn delay_lines_are_at_least_one_frame() {
    assert_eq!(delay_frames(0.0101, 44100.0), 446);
    assert_eq!(delay_frames(0.0, 44100.0), 1);
    assert_eq!(delay_frames(f32::NAN, 44100.0), 1);
    assert_eq!(delay_frames(1e6, 44100.0), 1);
}

#[test]
fn biquad_above_the_voice_nyquist_frequency_stays_bounded() {
    for filter_type in [
        BiquadType::Lowpass,
        BiquadType::Highpass,
        BiquadType::Bandpass,
    ] {
        let mut biquad = BiquadResonantFilter::default();
        biquad.set_params(filter_type, 5000.0, 2.0).unwrap();
        let mut instance = biquad.create_instance();
        let mut input = noise(8 * 512);
        for block in input.chunks_mut(512) {
            instance.filter(block, 512, 512, 1, 8820.0, 0.0);
        }
        assert!(input.iter().all(|x| x.abs() < 10.0), "{filter_type:?}");
    }
}

#[test]
fn biquad_lowpass_attenuates_high_frequencies() {
    let mut biquad = BiquadResonantFilter::default();
    biquad.set_params(BiquadType::Lowpass, 500.0, 0.7).unwrap();
    let tone: Vec<f32> = (0..4096)
        .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
        .collect();
    let out = run(&biquad, &tone, 1, 44100.0);
    assert!(out[2048..].iter().all(|x| x.abs() < 0.01));
}

#[test]
fn biquad_single_sample_is_left_dry() {
    let out = run(&BiquadResonantFilter::default(), &[0.5], 1, 44100.0);
    assert_eq!(out, [0.5]);
    let out = run(
        &BiquadResonantFilter::default(),
        &[0.5, 0.25, 0.75],
        1,
        44100.0,
    );
    assert_eq!(out[2], out[1]);
}

#[test]
fn lofi_filters_every_channel_alike() {
    let mut lofi = LofiFilter::default();
    lofi.set_params(8000.0, 4.0).unwrap();
    let one = noise(512);
    let eight: Vec<f32> = (0..8).flat_map(|_| one.iter().copied()).collect();
    let out = run(&lofi, &eight, 8, 44100.0);
    for channel in out.chunks(512) {
        assert_eq!(channel, &out[..512]);
    }
    assert_ne!(&out[..512], &one[..]);
}

#[test]
fn echo_repeats_after_the_delay() {
    let mut echo = EchoFilter::default();
    echo.set_params(0.001, 0.5, 0.0).unwrap();
    let mut impulse = vec![0.0; 512];
    impulse[0] = 1.0;
    // 0.001 s at 10 kHz is 10 frames.
    let out = run(&echo, &impulse, 1, 10_000.0);
    assert_eq!((out[0], out[10], out[20]), (1.0, 0.5, 0.25));
    assert_eq!(out[5], 0.0);
}

#[test]
fn echo_with_zero_delay_is_finite() {
    let mut instance = EchoFilter::default().create_instance();
    instance.params_mut().set(EchoFilter::DELAY, 0.0);
    let mut buffer = vec![0.25; 2 * 512];
    instance.filter(&mut buffer, 512, 512, 2, 44100.0, 0.0);
    assert!(buffer.iter().all(|x| x.is_finite()));
}

#[test]
fn waveshaper_minimum_amount_is_finite() {
    let mut shaper = WaveShaperFilter::default();
    shaper.set_params(-1.0).unwrap();
    let ramp: Vec<f32> = (0..512).map(|i| i as f32 / 256.0 - 1.0).collect();
    assert!(
        run(&shaper, &ramp, 1, 44100.0)
            .iter()
            .all(|x| x.is_finite())
    );
    let out = run(&WaveShaperFilter::default(), &ramp, 1, 44100.0);
    assert_eq!(out, ramp);
}

#[test]
fn dc_removal_removes_a_constant_offset() {
    let mut dc = DcRemovalFilter::default();
    dc.set_params(0.01).unwrap();
    let out = run(&dc, &[0.25; 2048], 1, 44100.0);
    assert!(out[1024..].iter().all(|x| x.abs() < 1e-6));
    dc.set_params(1e6).unwrap();
    assert!(
        run(&dc, &[0.25; 1024], 2, 44100.0)
            .iter()
            .all(|x| x.is_finite())
    );
}

#[test]
fn settings_reject_nan() {
    let nan = f32::NAN;
    let mut biquad = BiquadResonantFilter::default();
    assert!(biquad.set_params(BiquadType::Lowpass, nan, 2.0).is_err());
    assert!(biquad.set_params(BiquadType::Lowpass, 1000.0, nan).is_err());
    let mut lofi = LofiFilter::default();
    assert!(lofi.set_params(nan, 4.0).is_err() && lofi.set_params(8000.0, nan).is_err());
    let mut echo = EchoFilter::default();
    assert!(echo.set_params(nan, 0.5, 0.0).is_err());
    assert!(echo.set_params(0.1, nan, 0.0).is_err());
    assert!(echo.set_params(0.1, 0.5, nan).is_err() && echo.set_params(0.1, 0.5, 1.0).is_err());
    assert!(WaveShaperFilter::default().set_params(nan).is_err());
    assert!(DcRemovalFilter::default().set_params(nan).is_err());
    assert_eq!(echo, EchoFilter::default());
}

#[test]
fn parameter_descriptions_match_soloud() {
    let biquad = BiquadResonantFilter::default();
    assert_eq!(biquad.param_count(), 4);
    assert_eq!(biquad.param_name(3), Some("Resonance"));
    assert_eq!(biquad.param_name(4), None);
    assert_eq!(
        biquad.param_type(BiquadResonantFilter::TYPE),
        ParamType::Int
    );
    assert_eq!(biquad.param_max(BiquadResonantFilter::FREQUENCY), 8000.0);
    assert_eq!(EchoFilter::default().param_max(EchoFilter::DELAY), 0.3);
    assert_eq!(WaveShaperFilter::default().param_name(5), Some("Wet"));
    assert_eq!(WaveShaperFilter::default().param_min(1), -1.0);
    let dc = DcRemovalFilter::default();
    assert_eq!((dc.param_count(), dc.param_name(2)), (1, Some("Wet")));
    let instance = LofiFilter::default().create_instance();
    assert_eq!(instance.params().values(), &[1.0, 4000.0, 3.0]);
}
