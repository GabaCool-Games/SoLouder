//! Choosing a stream configuration the device supports and the mixer can
//! render.

use cpal::{SampleFormat, SupportedStreamConfig, SupportedStreamConfigRange};

/// Sample formats the output converts the mix to.
pub(crate) const SAMPLE_FORMATS: [SampleFormat; 9] = [
    SampleFormat::F32,
    SampleFormat::I16,
    SampleFormat::I32,
    SampleFormat::I24,
    SampleFormat::F64,
    SampleFormat::U16,
    SampleFormat::I8,
    SampleFormat::U8,
    SampleFormat::U32,
];

/// Channel counts the mixer renders (SoLoud's speaker layouts).
const MIXER_CHANNELS: [u16; 5] = [1, 2, 4, 6, 8];

/// What the caller would like; anything unset follows the device default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamPreferences {
    /// Output sample rate in Hz.
    pub sample_rate: Option<u32>,
    /// Output channels: 1, 2, 4, 6 or 8.
    pub channels: Option<u16>,
}

/// Picks a configuration: the device default when it satisfies the
/// preferences and the mixer, otherwise the closest supported range.
/// Ranges are ranked by channel count (the preferred one, else the default's,
/// else stereo), then by sample rate, then by sample format order in
/// [`SAMPLE_FORMATS`].
pub(crate) fn choose(
    default: &SupportedStreamConfig,
    ranges: &[SupportedStreamConfigRange],
    preferences: StreamPreferences,
) -> Option<SupportedStreamConfig> {
    let rate = preferences.sample_rate.unwrap_or(default.sample_rate());
    let wanted_channels = preferences.channels.unwrap_or(default.channels());
    let default_fits = default.sample_rate() == rate
        && default.channels() == wanted_channels
        && MIXER_CHANNELS.contains(&default.channels())
        && SAMPLE_FORMATS.contains(&default.sample_format());
    if default_fits {
        return Some(*default);
    }

    let channel_rank = |channels: u16| {
        if channels == wanted_channels {
            0
        } else if preferences.channels.is_none() && channels == 2 {
            1
        } else {
            2 + channels.abs_diff(wanted_channels) as u32
        }
    };
    let format_rank = |format: SampleFormat| {
        SAMPLE_FORMATS
            .iter()
            .position(|&f| f == format)
            .unwrap_or(usize::MAX)
    };
    ranges
        .iter()
        .filter(|range| MIXER_CHANNELS.contains(&range.channels()))
        .filter(|range| SAMPLE_FORMATS.contains(&range.sample_format()))
        .filter(|range| preferences.channels.is_none_or(|c| c == range.channels()))
        .map(|range| {
            let clamped = rate.clamp(range.min_sample_rate(), range.max_sample_rate());
            let rate_rank = clamped.abs_diff(rate);
            let rank = (
                channel_rank(range.channels()),
                rate_rank,
                format_rank(range.sample_format()),
            );
            (rank, (*range).with_sample_rate(clamped))
        })
        .filter(|(_, config)| {
            preferences
                .sample_rate
                .is_none_or(|r| r == config.sample_rate())
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, config)| config)
}

#[cfg(test)]
mod tests {
    use cpal::SupportedBufferSize;

    use super::*;

    fn config(channels: u16, rate: u32, format: SampleFormat) -> SupportedStreamConfig {
        SupportedStreamConfig::new(channels, rate, SupportedBufferSize::Unknown, format)
    }

    fn range(
        channels: u16,
        min: u32,
        max: u32,
        format: SampleFormat,
    ) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(channels, min, max, SupportedBufferSize::Unknown, format)
    }

    #[test]
    fn keeps_a_usable_default() {
        let default = config(2, 48000, SampleFormat::F32);
        let chosen = choose(&default, &[], StreamPreferences::default()).unwrap();
        assert_eq!(chosen, default);
    }

    #[test]
    fn avoids_channel_counts_the_mixer_cannot_render() {
        let default = config(3, 48000, SampleFormat::F32);
        let ranges = [
            range(3, 48000, 48000, SampleFormat::F32),
            range(2, 44100, 48000, SampleFormat::I16),
            range(8, 48000, 48000, SampleFormat::F32),
        ];
        let chosen = choose(&default, &ranges, StreamPreferences::default()).unwrap();
        assert_eq!(chosen, config(2, 48000, SampleFormat::I16));
    }

    #[test]
    fn honours_preferences_or_fails() {
        let default = config(2, 48000, SampleFormat::F32);
        let ranges = [
            range(2, 8000, 96000, SampleFormat::F32),
            range(6, 48000, 48000, SampleFormat::I16),
        ];
        let wants = |sample_rate, channels| StreamPreferences {
            sample_rate,
            channels,
        };
        assert_eq!(
            choose(&default, &ranges, wants(Some(44100), None)),
            Some(config(2, 44100, SampleFormat::F32))
        );
        assert_eq!(
            choose(&default, &ranges, wants(None, Some(6))),
            Some(config(6, 48000, SampleFormat::I16))
        );
        assert_eq!(choose(&default, &ranges, wants(Some(44100), Some(6))), None);
        assert_eq!(choose(&default, &ranges, wants(None, Some(4))), None);
    }

    #[test]
    fn prefers_float_among_equal_ranges() {
        let default = config(2, 48000, SampleFormat::I64);
        let ranges = [
            range(2, 48000, 48000, SampleFormat::U8),
            range(2, 48000, 48000, SampleFormat::F32),
            range(2, 48000, 48000, SampleFormat::I16),
        ];
        let chosen = choose(&default, &ranges, StreamPreferences::default()).unwrap();
        assert_eq!(chosen.sample_format(), SampleFormat::F32);
    }
}
