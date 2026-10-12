//! Scenario operations on filters. Each mirrors the operation of the same
//! name in `reference/tests/parity_runner.cpp`.

use std::sync::Arc;

use solouder_core::filters::{
    BiquadResonantFilter, BiquadType, DcRemovalFilter, EchoFilter, Filter, LofiFilter, ParamType,
    WaveShaperFilter,
};
use solouder_core::{FilterTarget, Handle};

use crate::ops::{code, hex32};
use crate::run::State;
use crate::scenario::{ScenarioError, Step};

impl State {
    /// A new filter of `kind`. When any of the kind's settings is given, they
    /// go to its `set_params`; missing ones take the defaults.
    pub(crate) fn filter(&mut self, step: &Step) -> Result<(), ScenarioError> {
        let kind = step.text("kind")?;
        let settings: &[&str] = match kind {
            "biquad" => &["type", "freq", "res"],
            "lofi" => &["rate", "bits"],
            "echo" => &["delay", "decay", "filter"],
            "waveshaper" => &["amount"],
            "dc_removal" => &["length"],
            other => return Err(step.error(format!("unknown filter kind `{other}`"))),
        };
        let mut keys = vec!["name", "kind"];
        keys.extend_from_slice(settings);
        step.check_keys(&keys)?;
        step.text("name")?;
        let configure = settings.iter().any(|key| step.get(key).is_some());
        let (filter, result): (Arc<dyn Filter>, _) = match kind {
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
            _ => {
                let length = step.number_or("length", 0.1)?;
                let mut dc = DcRemovalFilter::default();
                let result = configure.then(|| dc.set_params(length));
                (Arc::new(dc), result)
            }
        };
        let name = step.text("name")?.to_owned();
        self.filters.insert(name.clone(), filter);
        let code = code(result.unwrap_or(Ok(())));
        self.log(format!("filter {name} {code}"));
        Ok(())
    }

    /// `AudioSource::setFilter`; without `filter` the slot is cleared.
    pub(crate) fn sound_filter(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["sound", "slot", "filter"])?;
        let name = step.text("sound")?;
        if !self.sounds.contains_key(name) {
            return Err(step.error(format!("no sound named `{name}`")));
        }
        let slot: u32 = step.value("slot")?;
        let filter = self.named_filter(step, "filter")?;
        if let Some(sound) = self.sounds.get_mut(name) {
            sound.set_filter(slot, filter);
        }
        Ok(())
    }

    /// `Soloud::setGlobalFilter`; without `filter` the slot is cleared.
    pub(crate) fn global_filter(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["slot", "filter"])?;
        let slot: u32 = step.value("slot")?;
        let filter = self.named_filter(step, "filter")?;
        self.mixer(step)?.set_global_filter(slot, filter.as_deref());
        Ok(())
    }

    /// `filter_set`, `filter_fade`, `filter_oscillate` and `filter_query`;
    /// without `h` they address the global filters (handle 0).
    pub(crate) fn filter_parameter(&mut self, step: &Step) -> Result<(), ScenarioError> {
        let keys: &[&str] = match step.op.as_str() {
            "filter_set" => &["h", "slot", "param", "value"],
            "filter_fade" => &["h", "slot", "param", "to", "time"],
            "filter_oscillate" => &["h", "slot", "param", "from", "to", "time"],
            _ => &["h", "slot", "param"],
        };
        step.check_keys(keys)?;
        let handle = match step.get("h") {
            Some(_) => self.handle(step)?,
            None => Handle::INVALID,
        };
        // Handle 0 addresses the global filters in the C API.
        let target = if handle == Handle::INVALID {
            FilterTarget::Global
        } else {
            FilterTarget::Voice(handle)
        };
        let slot: u32 = step.value("slot")?;
        let param: u32 = step.value("param")?;
        self.mixer(step)?;
        for key in &keys[3..] {
            step.text(key)?;
        }
        let (from, to) = (step.number_or("from", 0.0)?, step.number_or("to", 0.0)?);
        let (value, time) = (step.number_or("value", 0.0)?, step.seconds_or("time", 0.0)?);
        let name = step.get("h").unwrap_or("global").to_owned();
        let mixer = self.mixer(step)?;
        match step.op.as_str() {
            "filter_set" => mixer.set_filter_parameter(target, slot, param, value),
            "filter_fade" => mixer.fade_filter_parameter(target, slot, param, to, time),
            "filter_oscillate" => {
                mixer.oscillate_filter_parameter(target, slot, param, from, to, time);
            }
            _ => {
                // SoLoud returns INVALID_PARAMETER (1.0) without a filter and 0
                // past the last parameter.
                let value = mixer
                    .filter_parameter(target, slot, param)
                    .unwrap_or_else(|| {
                        let exists = mixer.filter_parameter(target, slot, 0).is_some();
                        if exists { 0.0 } else { 1.0 }
                    });
                self.log(format!(
                    "filter_query {name} {slot} {param} {}",
                    hex32(value)
                ));
            }
        }
        Ok(())
    }

    /// Logs the filter's parameter descriptions, up to one past the last.
    pub(crate) fn filter_info(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["name"])?;
        let filter = self
            .named_filter(step, "name")?
            .ok_or_else(|| step.error("`filter_info` needs `name=`"))?;
        let count = filter.param_count();
        let mut line = format!("filter_info {} count={count}", step.text("name")?);
        for i in 0..=count {
            let kind = match filter.param_type(i) {
                ParamType::Float => 0,
                ParamType::Int => 1,
                ParamType::Bool => 2,
            };
            line += &format!(
                " {i}:{}:{kind}:{}:{}",
                filter.param_name(i).unwrap_or("-"),
                hex32(filter.param_max(i)),
                hex32(filter.param_min(i))
            );
        }
        self.log(line);
        Ok(())
    }

    /// The filter named by `key`, or `None` when the key is absent.
    fn named_filter(
        &self,
        step: &Step,
        key: &str,
    ) -> Result<Option<Arc<dyn Filter>>, ScenarioError> {
        let Some(name) = step.get(key) else {
            return Ok(None);
        };
        self.filters
            .get(name)
            .cloned()
            .map(Some)
            .ok_or_else(|| step.error(format!("no filter named `{name}`")))
    }
}
