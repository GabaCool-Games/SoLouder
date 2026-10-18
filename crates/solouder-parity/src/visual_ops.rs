//! Visualization: the `visual` operation, on the mixer and in the real-time
//! replay, and the replay's `bus_wave` and `bus_set`. Mirrors the `visual`
//! branch of `reference/tests/parity_runner.cpp`.

use crate::bus_ops::resampler;
use crate::realtime::{Outcome, Replay, stop};
use crate::run::State;
use crate::scenario::{ScenarioError, Step};

/// What `visual` appends to the samples.
#[derive(Clone, Copy)]
enum Data {
    /// `getWave`.
    Wave,
    /// `calcFFT`.
    Fft,
    /// `getApproximateVolume` of channels 0 to 9.
    Volumes,
}

fn data(step: &Step) -> Result<Data, ScenarioError> {
    step.check_keys(&["data"])?;
    match step.text("data")? {
        "wave" => Ok(Data::Wave),
        "fft" => Ok(Data::Fft),
        "volumes" => Ok(Data::Volumes),
        other => Err(step.error(format!("unknown visual data `{other}`"))),
    }
}

impl State {
    /// Appends the mixer's visualization data to the samples.
    pub(crate) fn visual(&mut self, step: &Step) -> Result<(), ScenarioError> {
        let data = data(step)?;
        let mixer = self.mixer(step)?;
        let values = match data {
            Data::Wave => mixer.wave().to_vec(),
            Data::Fft => mixer.fft().to_vec(),
            Data::Volumes => (0..10).map(|c| mixer.approximate_volume(c)).collect(),
        };
        self.rendered.samples.extend(values);
        self.log(format!("visual {}", step.text("data")?));
        Ok(())
    }
}

impl Replay {
    /// `visual` through the controller, which reads what the renderer
    /// published at the last render.
    pub(crate) fn visual(&mut self, step: &Step) -> Outcome {
        let data = data(step)?;
        let engine = match self.engine() {
            Ok(engine) => engine,
            Err(reason) => return Ok(reason),
        };
        let c = &mut engine.controller;
        let values = match data {
            Data::Wave => c.wave().to_vec(),
            Data::Fft => c.fft().to_vec(),
            Data::Volumes => (0..10).map(|ch| c.approximate_volume(ch)).collect(),
        };
        self.samples.extend(values);
        Ok(None)
    }

    /// `bus_wave` through the controller: nothing when the bus was not
    /// playing, as the mixer logs `none`. The controller publishes only
    /// buses whose visualization it enabled, so the replay stops at a bus
    /// that does not record (one turned off may still be published; the
    /// replay does not track that).
    pub(crate) fn bus_wave(&mut self, step: &Step) -> Outcome {
        step.check_keys(&["bus", "fft"])?;
        let bus = self.defs.bus_named(step, "bus")?;
        if !bus.visualization() {
            return stop("the controller publishes only buses that record");
        }
        let fft = step.flag_or("fft", false)?;
        let engine = match self.engine() {
            Ok(engine) => engine,
            Err(reason) => return Ok(reason),
        };
        let c = &mut engine.controller;
        let data = if fft {
            c.bus_fft(&bus)
        } else {
            c.bus_wave(&bus)
        };
        self.samples.extend(data.into_iter().flatten());
        Ok(None)
    }

    /// `bus_set`: the bus definition changes, and a playing bus's resampler
    /// and visualization through the controller.
    pub(crate) fn bus_set(&mut self, step: &Step) -> Outcome {
        self.defs.apply(step)?;
        let mut bus = self.defs.bus_named(step, "bus")?;
        let value: Option<u32> = step
            .get("resampler")
            .map(|_| step.value("resampler"))
            .transpose()?;
        let visualization = step
            .get("visualization")
            .map(|_| step.flag_or("visualization", false))
            .transpose()?;
        self.engine_op(|c| {
            if let Some(resampler) = value.and_then(resampler) {
                c.set_bus_resampler(&mut bus, resampler)?;
            }
            if let Some(enable) = visualization {
                c.set_bus_visualization(&mut bus, enable)?;
            }
            Ok(())
        })
    }
}
