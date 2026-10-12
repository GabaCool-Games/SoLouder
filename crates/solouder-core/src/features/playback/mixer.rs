use super::config::{ConfigError, MixerConfig};

/// The SoLouder mixer: renders interleaved `f32` output.
///
/// Corresponds to an initialized SoLoud engine driven by explicit `mix` calls
/// (the null driver). Device output adapters own a `Mixer` and call
/// [`Mixer::mix_interleaved`] from their callback.
#[derive(Debug)]
pub struct Mixer {
    config: MixerConfig,
}

impl Mixer {
    /// Creates a mixer, rejecting configurations SoLoud would refuse.
    pub fn new(config: MixerConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self { config })
    }

    /// The configuration the mixer was created with.
    pub fn config(&self) -> MixerConfig {
        self.config
    }

    /// Renders `output.len() / channels` frames of interleaved audio.
    ///
    /// Any trailing partial frame is set to silence. Unlike SoLoud's
    /// `Soloud::mix`, any number of frames is accepted: SoLoud writes past its
    /// scratch buffer when asked for more frames than it holds.
    ///
    /// No audio sources exist yet, so the output is silence.
    pub fn mix_interleaved(&mut self, output: &mut [f32]) {
        output.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_invalid_config() {
        let config = MixerConfig {
            channels: 3,
            ..MixerConfig::default()
        };
        assert_eq!(
            Mixer::new(config).unwrap_err(),
            ConfigError::UnsupportedChannelCount(3)
        );
    }

    #[test]
    fn mix_without_sources_is_silent() {
        let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
        let mut output = vec![1.0_f32; 2 * 600];
        mixer.mix_interleaved(&mut output);
        assert!(output.iter().all(|&sample| sample == 0.0));
    }

    #[test]
    fn mix_accepts_empty_and_oversized_buffers() {
        let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
        mixer.mix_interleaved(&mut []);
        let mut large = vec![1.0_f32; 2 * 10_000];
        mixer.mix_interleaved(&mut large);
        assert!(large.iter().all(|&sample| sample == 0.0));
    }
}
