//! The `fft` scenario operation; mirrors `fftDump` in
//! `reference/tests/parity_runner.cpp`.

use solouder_core::dsp::fft;

use crate::pattern::Pattern;
use crate::run::State;
use crate::scenario::{ScenarioError, Step};

impl State {
    /// Transforms `size` floats of a test signal and appends the result to
    /// the samples.
    pub(crate) fn fft(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["size", "inverse", "pattern", "amp", "period", "seed"])?;
        let size: u32 = step.value("size")?;
        if !(4..=4096).contains(&size) || !size.is_power_of_two() {
            return Err(step.error("`size` must be a power of two from 4 to 4096"));
        }
        let inverse = step.flag_or("inverse", false)?;
        let amp = step.number_or("amp", 1.0)?;
        let mut buffer = Pattern::from_step(step)?.render(1, size, amp);
        if inverse {
            fft::ifft(&mut buffer);
        } else {
            fft::fft(&mut buffer);
        }
        self.rendered.samples.extend_from_slice(&buffer);
        Ok(())
    }
}
