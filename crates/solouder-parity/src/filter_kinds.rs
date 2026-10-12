//! The filter kinds the `filter` operation creates; mirrors `createFilter`
//! in `reference/tests/parity_runner.cpp`.

use std::sync::Arc;

use solouder_core::filters::{
    BassboostFilter, BiquadResonantFilter, BiquadType, DcRemovalFilter, EchoFilter, FftFilter,
    Filter, FlangerFilter, FreeverbFilter, InvalidFilterParams, LofiFilter, RobotizeFilter,
    WaveShaperFilter,
};

use crate::scenario::{ScenarioError, Step};

/// A filter and, when any setting was given, the result of its `set_params`.
pub(crate) type Built = (Arc<dyn Filter>, Option<Result<(), InvalidFilterParams>>);

/// The settings `kind` accepts.
pub(crate) fn settings(step: &Step, kind: &str) -> Result<&'static [&'static str], ScenarioError> {
    Ok(match kind {
        "biquad" => &["type", "freq", "res"],
        "lofi" => &["rate", "bits"],
        "echo" => &["delay", "decay", "filter"],
        "waveshaper" => &["amount"],
        "dc_removal" => &["length"],
        "fft" => &[],
        "bassboost" => &["boost"],
        "robotize" => &["freq", "wave"],
        "flanger" => &["delay", "freq"],
        "freeverb" => &["freeze", "room", "damp", "width"],
        other => return Err(step.error(format!("unknown filter kind `{other}`"))),
    })
}

/// A new filter of `kind`; `configure` passes the settings to `set_params`,
/// with defaults for the missing ones.
pub(crate) fn build(step: &Step, kind: &str, configure: bool) -> Result<Built, ScenarioError> {
    Ok(match kind {
        "biquad" => {
            let filter_type = match step.get("type").unwrap_or("lowpass") {
                "lowpass" => BiquadType::Lowpass,
                "highpass" => BiquadType::Highpass,
                "bandpass" => BiquadType::Bandpass,
                other => return Err(step.error(format!("unknown biquad type `{other}`"))),
            };
            let (freq, res) = (step.number_or("freq", 1000.0)?, step.number_or("res", 2.0)?);
            let mut biquad = BiquadResonantFilter::default();
            let result = configure.then(|| biquad.set_params(filter_type, freq, res));
            (Arc::new(biquad), result)
        }
        "lofi" => {
            let (rate, bits) = (
                step.number_or("rate", 4000.0)?,
                step.number_or("bits", 3.0)?,
            );
            let mut lofi = LofiFilter::default();
            let result = configure.then(|| lofi.set_params(rate, bits));
            (Arc::new(lofi), result)
        }
        "echo" => {
            let delay = step.number_or("delay", 0.3)?;
            let decay = step.number_or("decay", 0.7)?;
            let lowpass = step.number_or("filter", 0.0)?;
            let mut echo = EchoFilter::default();
            let result = configure.then(|| echo.set_params(delay, decay, lowpass));
            (Arc::new(echo), result)
        }
        "waveshaper" => {
            let amount = step.number_or("amount", 0.0)?;
            let mut shaper = WaveShaperFilter::default();
            let result = configure.then(|| shaper.set_params(amount));
            (Arc::new(shaper), result)
        }
        "dc_removal" => {
            let length = step.number_or("length", 0.1)?;
            let mut dc = DcRemovalFilter::default();
            let result = configure.then(|| dc.set_params(length));
            (Arc::new(dc), result)
        }
        "fft" => (Arc::new(FftFilter), None),
        "bassboost" => {
            let boost = step.number_or("boost", 2.0)?;
            let mut bass = BassboostFilter::default();
            let result = configure.then(|| bass.set_params(boost));
            (Arc::new(bass), result)
        }
        "robotize" => {
            let freq = step.number_or("freq", 30.0)?;
            let wave: u32 = step.value_or("wave", 0)?;
            let mut robot = RobotizeFilter::default();
            if configure {
                robot.set_params(freq, wave as i32);
            }
            (Arc::new(robot), None)
        }
        "flanger" => {
            let (delay, freq) = (
                step.number_or("delay", 0.005)?,
                step.number_or("freq", 10.0)?,
            );
            let mut flanger = FlangerFilter::default();
            let result = configure.then(|| flanger.set_params(delay, freq));
            (Arc::new(flanger), result)
        }
        _ => {
            let freeze = step.number_or("freeze", 0.0)?;
            let room = step.number_or("room", 0.5)?;
            let damp = step.number_or("damp", 0.5)?;
            let width = step.number_or("width", 1.0)?;
            let mut reverb = FreeverbFilter::default();
            let result = configure.then(|| reverb.set_params(freeze, room, damp, width));
            (Arc::new(reverb), result)
        }
    })
}
