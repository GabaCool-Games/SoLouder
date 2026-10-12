// Ported from SoLoud src/core/soloud_fader.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

/// What a [`Fader`] is doing (`Fader::mActive`: 0, 1, 2 and -1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FaderState {
    #[default]
    Off,
    /// Moving linearly from `from` to `to`.
    Linear,
    /// Oscillating between `from` and `to`.
    Lfo,
    /// A linear fade that has reached `to`; `get` keeps returning it.
    Finished,
}

/// A value that changes over time: a linear fade or a sine oscillation.
/// Times are in seconds, as `f64`; values are `f32`, as in SoLoud.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Fader {
    pub state: FaderState,
    from: f32,
    to: f32,
    delta: f32,
    time: f64,
    start_time: f64,
    /// End time for a fade; angular speed for an oscillation.
    end_time: f64,
    /// The last value of a fade, for restarting after time goes backwards.
    current: f32,
}

impl Fader {
    /// Fades or oscillates (`mActive > 0`).
    pub fn is_running(&self) -> bool {
        matches!(self.state, FaderState::Linear | FaderState::Lfo)
    }

    /// The highest value a running fade or oscillation reaches.
    pub fn highest(&self) -> Option<f32> {
        self.is_running().then(|| self.from.max(self.to))
    }

    /// Starts a fade from `from` to `to` over `time` seconds, from `start_time`.
    pub fn set(&mut self, from: f32, to: f32, time: f64, start_time: f64) {
        // SoLoud takes the previous fade's start value here.
        self.current = self.from;
        self.from = from;
        self.to = to;
        self.time = time;
        self.start_time = start_time;
        self.delta = to - from;
        self.end_time = start_time + time;
        self.state = FaderState::Linear;
    }

    /// Starts a sine oscillation between `from` and `to` with a period of
    /// `time` seconds, from `start_time`.
    pub fn set_lfo(&mut self, from: f32, to: f32, time: f64, start_time: f64) {
        self.state = FaderState::Lfo;
        self.current = 0.0;
        self.from = from;
        self.to = to;
        self.time = time;
        // Signed, so the oscillation centres on the midpoint even when `from`
        // is above `to` (reference patch 0051).
        self.delta = (to - from) / 2.0;
        self.start_time = start_time;
        self.end_time = f64::from(std::f32::consts::PI * 2.0) / time;
    }

    /// The value at `now`. A fade past its end becomes
    /// [`FaderState::Finished`] and returns `to`.
    pub fn get(&mut self, now: f64) -> f32 {
        if self.state == FaderState::Lfo {
            if self.start_time > now {
                self.start_time = now;
            }
            let t = now - self.start_time;
            return ((t * self.end_time).sin() * f64::from(self.delta)
                + f64::from(self.from + self.delta)) as f32;
        }
        if self.start_time > now {
            // Time went backwards: continue from the last value.
            let progress = (self.current - self.from) / self.delta;
            self.from = self.current;
            self.start_time = now;
            self.time *= f64::from(1.0 - progress);
            self.delta = self.to - self.from;
            self.end_time = self.start_time + self.time;
        }
        if now > self.end_time {
            self.state = FaderState::Finished;
            return self.to;
        }
        self.current = (f64::from(self.from)
            + f64::from(self.delta) * ((now - self.start_time) / self.time))
            as f32;
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fade_moves_linearly_then_holds_its_target() {
        let mut fader = Fader::default();
        fader.set(0.0, 1.0, 2.0, 10.0);
        assert_eq!(fader.get(10.5), 0.25);
        assert_eq!(fader.get(12.0), 1.0);
        assert_eq!(fader.state, FaderState::Linear);
        assert_eq!(fader.get(12.5), 1.0);
        assert_eq!(fader.state, FaderState::Finished);
        assert!(!fader.is_running());
    }

    #[test]
    fn an_oscillation_swings_between_its_bounds() {
        let mut fader = Fader::default();
        fader.set_lfo(0.2, 0.8, 1.0, 0.0);
        assert!((fader.get(0.0) - 0.5).abs() < 1e-6);
        assert!((fader.get(0.25) - 0.8).abs() < 1e-6);
        assert!((fader.get(0.75) - 0.2).abs() < 1e-6);
        assert!(fader.is_running());
    }
}
