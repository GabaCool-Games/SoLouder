// Ported from SoLoud src/core/soloud.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::voice::{Voice, flags};
use super::voices::{BLOCK_LEN, Voices};
use crate::contracts::SAMPLE_GRANULARITY;
use crate::dsp::Resampler;
use crate::dsp::pan::{Gains, pan_and_expand};
use crate::dsp::resample::{FRAC_BITS, FRAC_MUL};

const BLOCK: usize = SAMPLE_GRANULARITY as usize;
/// One whole source block in fixed point.
const BLOCK_FIXED: u32 = SAMPLE_GRANULARITY * FRAC_MUL;

/// How a bus mixes: planar buffers with `stride` frames per channel, of which
/// the first `frames` are rendered.
#[derive(Clone, Copy)]
pub(crate) struct Layout {
    pub frames: u32,
    pub stride: usize,
    /// Channels of the accumulator.
    pub channels: usize,
    pub sample_rate: f32,
    pub resampler: Resampler,
}

/// Mixes every active, unpaused voice into the accumulator `out`, using
/// `scratch` (up to `MAX_CHANNELS` planar channels) for each voice's
/// resampled audio, and stops voices that have ended
/// (`Soloud::mixBus_internal`).
pub(crate) fn mix_bus(voices: &mut Voices, out: &mut [f32], scratch: &mut [f32], bus: Layout) {
    let frames = bus.frames as usize;
    for k in 0..bus.channels {
        out[k * bus.stride..k * bus.stride + frames].fill(0.0);
    }
    for i in 0..voices.active_count {
        let slot = voices.active[i] as usize;
        let Some(voice) = voices.slots[slot].as_mut() else {
            continue;
        };
        if voice.has_flag(flags::PAUSED) {
            continue;
        }
        resample_voice(voice, &mut voices.pool.data, scratch, bus);
        let source_channels = voice.channels();
        let gains = Gains {
            current: &mut voice.current_channel_volume,
            channel_volume: &voice.channel_volume,
            overall_volume: voice.overall_volume,
        };
        pan_and_expand(
            gains,
            source_channels,
            out,
            frames,
            bus.stride,
            scratch,
            bus.channels,
        );
        let keeps_playing = voice.has_flag(flags::LOOPING | flags::DISABLE_AUTOSTOP);
        if !keeps_playing && voice.has_ended() {
            voices.stop(slot);
        }
    }
}

/// Renders `bus.frames` frames of `voice` at the bus rate into `scratch`,
/// pulling 512-frame source blocks into the voice's resample blocks as needed.
fn resample_voice(voice: &mut Voice, pool: &mut [f32], scratch: &mut [f32], bus: Layout) {
    let (frames, stride) = (bus.frames, bus.stride);
    let channels = voice.channels();
    let mut step = voice.sample_rate / bus.sample_rate;
    // A step of a whole block or more cannot advance (reference patch 0022).
    if step.is_nan() || step >= SAMPLE_GRANULARITY as f32 {
        step = 0.0;
    }
    let step_fixed = (step * FRAC_MUL as f32).floor() as u32;
    let Some(mut blocks) = voice.resample else {
        // Not reachable: active voices always have blocks. Render silence.
        return clear_tail(scratch, channels, stride, 0, frames);
    };

    let mut written = 0;
    if voice.delay_samples != 0 {
        written = voice.delay_samples.min(frames);
        voice.delay_samples -= written;
        clear_tail(scratch, channels, stride, 0, written);
    }

    while step_fixed != 0 && written < frames {
        if voice.leftover_samples == 0 {
            std::mem::swap(&mut blocks.current, &mut blocks.previous);
            let block = &mut pool[blocks.current * BLOCK_LEN..][..BLOCK_LEN];
            let read = fill_block(voice, block);
            if read < SAMPLE_GRANULARITY {
                for c in 0..channels {
                    block[c * BLOCK + read as usize..(c + 1) * BLOCK].fill(0.0);
                }
            }
            // The new block starts a block later: move the position back.
            voice.src_offset = voice.src_offset.saturating_sub(BLOCK_FIXED);
        } else {
            voice.leftover_samples = 0;
        }

        // Frames whose position stays inside this block. The position stays
        // below 2^31: step_fixed < BLOCK_FIXED = 2^29.
        let mut count = 0;
        if voice.src_offset < BLOCK_FIXED {
            count = (BLOCK_FIXED - voice.src_offset) / step_fixed + 1;
            if (count * step_fixed + voice.src_offset) >> FRAC_BITS >= SAMPLE_GRANULARITY {
                count -= 1;
            }
        }
        if count + written > frames {
            voice.leftover_samples = count + written - frames;
            count = frames - written;
        }

        if count != 0 {
            let current = &pool[blocks.current * BLOCK_LEN..][..BLOCK_LEN];
            let previous = &pool[blocks.previous * BLOCK_LEN..][..BLOCK_LEN];
            for c in 0..channels {
                let dst = &mut scratch[c * stride + written as usize..][..count as usize];
                let src = &current[c * BLOCK..][..BLOCK];
                let prev = &previous[c * BLOCK..][..BLOCK];
                bus.resampler
                    .run(src, prev, dst, voice.src_offset, step_fixed);
            }
        }
        written += count;
        voice.src_offset += count * step_fixed;
    }
    voice.resample = Some(blocks);
    clear_tail(scratch, channels, stride, written, frames);
}

/// Reads the next source block, wrapping to the loop point as often as needed
/// for a looping voice. Returns the frames read.
fn fill_block(voice: &mut Voice, block: &mut [f32]) -> u32 {
    let looping = voice.has_flag(flags::LOOPING);
    if voice.has_ended() && !looping {
        return 0;
    }
    let mut read = voice.read(block, 0, SAMPLE_GRANULARITY);
    while looping && read < SAMPLE_GRANULARITY {
        voice.seek_to_loop_point();
        // `mLoopCount` is unsigned and wraps.
        voice.loop_count = voice.loop_count.wrapping_add(1);
        let more = voice.read(block, read as usize, SAMPLE_GRANULARITY - read);
        read += more;
        if more == 0 {
            break;
        }
    }
    read
}

/// Zeroes frames `from..to` of each scratch channel, so frames a voice did
/// not render are silent rather than another voice's data (patch 0008).
fn clear_tail(scratch: &mut [f32], channels: usize, stride: usize, from: u32, to: u32) {
    if from < to {
        for c in 0..channels {
            scratch[c * stride + from as usize..c * stride + to as usize].fill(0.0);
        }
    }
}
