// Ported from SoLoud src/audiosource/monotone/soloud_monotone.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! One voice of a Monotone song (`MonotoneInstance`).

use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::Ordering;

use solouder_core::dsp::{c_int, waveform};
use solouder_core::{GeneratorContext, GeneratorInstance};

use super::Live;
use super::song::{MonotoneSong, ROWS};

/// Entries of the note table: 8 a note, notes 0 to 99.
const NOTE_ENTRIES: usize = 800;

/// Each note's frequency in Hz, in eighths of a semitone from 27.5 Hz at
/// entry 8 (`Monotone::mNotesHz`). Entry 0 is 440; entries 1 to 7 go
/// below 27.5 Hz, down to 19 at most (entry 1 is reference patch 0080).
static NOTES_HZ: [i32; NOTE_ENTRIES] = notes_hz();

const fn notes_hz() -> [i32; NOTE_ENTRIES] {
    // About 2^(1/96), the interval of an eighth of a semitone; SoLoud's
    // literal, which rounds to the same `f32`.
    #[allow(clippy::excessive_precision)]
    const INTERVAL: f32 = 1.00724641222;
    let mut table = [0; NOTE_ENTRIES];
    table[0] = 440;
    // `floor(x + 0.5)`: every frequency is positive, so truncating is
    // the floor.
    table[8] = (27.5_f32 + 0.5) as i32;
    let mut hz = 27.5_f32;
    let mut i = 7;
    while i > 0 {
        hz /= INTERVAL;
        if hz < 19.0 {
            hz = 19.0;
        }
        table[i] = (hz + 0.5) as i32;
        i -= 1;
    }
    hz = 27.5;
    let mut i = 9;
    while i < NOTE_ENTRIES {
        hz *= INTERVAL;
        table[i] = (hz + 0.5) as i32;
        i += 1;
    }
    table
}

/// The vibrato's sine, -64 to 64 (`Monotone::mVibTable`).
static VIBRATO: LazyLock<[i32; 32]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let angle = i as f64 * std::f64::consts::PI / 32.0 * 2.0;
        (0.5 + 64.0 * angle.sin()).floor() as i32
    })
});

/// The note table's entry `index`, or its last past the end (reference
/// patch 0079; SoLoud read past the table for notes over 99).
fn note_hz(index: i32) -> i32 {
    NOTES_HZ[index.clamp(0, NOTE_ENTRIES as i32 - 1) as usize]
}

/// A track's state (`MonotoneChannel`).
#[derive(Clone, Copy, Debug)]
struct Channel {
    active: bool,
    /// The note's frequency and the arpeggio's two others; 0 at the start
    /// (reference patch 0082; SoLoud left them uninitialised).
    freq: [i32; 3],
    portamento: i32,
    arp_counter: usize,
    arp: bool,
    last_note: u32,
    portamento_to_note: i32,
    vibrato: bool,
    vibrato_index: i32,
    vibrato_depth: i32,
    vibrato_speed: i32,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            active: false,
            freq: [0; 3],
            portamento: 0,
            arp_counter: 0,
            arp: false,
            last_note: 0,
            portamento_to_note: 0,
            vibrato: false,
            vibrato_index: 0,
            vibrato_depth: 1,
            vibrato_speed: 1,
        }
    }
}

/// An oscillator (`MonotoneHardwareChannel`).
#[derive(Clone, Copy, Debug, Default)]
struct Output {
    pos: f32,
    step: f32,
}

/// One voice of a [`super::Monotone`]: a tracker reading a row every
/// `tempo` ticks of a 60th of a second, and hardware channels playing the
/// active tracks, taking turns when there are more tracks than channels.
#[derive(Debug)]
pub(crate) struct MonotoneInstance {
    song: Arc<MonotoneSong>,
    live: Arc<Live>,
    channels: [Channel; 12],
    outputs: [Output; 12],
    next_channel: usize,
    /// Ticks a row, 4 until a song sets it.
    tempo: i32,
    order: usize,
    row: usize,
    sample_count: i32,
    row_tick: i32,
}

impl MonotoneInstance {
    pub(crate) fn new(song: Arc<MonotoneSong>, live: Arc<Live>) -> Self {
        LazyLock::force(&VIBRATO);
        Self {
            song,
            live,
            channels: [Channel::default(); 12],
            outputs: [Output::default(); 12],
            next_channel: 0,
            tempo: 4,
            order: 0,
            row: 0,
            sample_count: 0,
            row_tick: 0,
        }
    }

    /// A tick: maybe a row, then the tracks' effects, then which tracks the
    /// hardware channels play until the next tick.
    fn tick(&mut self, rate: f32, hardware: usize) {
        self.row_tick += 1;
        if self.row_tick >= self.tempo {
            self.row_tick = 0;
            self.row();
        }
        let tracks = usize::from(self.song.tracks());
        for channel in &mut self.channels[..tracks] {
            if !channel.active {
                continue;
            }
            if channel.vibrato {
                let depth = VIBRATO[channel.vibrato_index as usize] * channel.vibrato_depth;
                channel.freq[0] = note_hz(channel.last_note as i32 * 8 + depth / 64);
                channel.vibrato_index = (channel.vibrato_index + channel.vibrato_speed) % 32;
            }
            if channel.portamento != 0 && self.row_tick != 0 {
                channel.freq[0] = channel.freq[0].wrapping_add(channel.portamento);
                let target = channel.portamento_to_note;
                if target != 0
                    && ((channel.portamento > 0 && channel.freq[0] >= target)
                        || (channel.portamento < 0 && channel.freq[0] <= target))
                {
                    channel.freq[0] = target;
                    channel.portamento_to_note = 0;
                }
            }
        }
        for output in &mut self.outputs[..hardware] {
            output.step = 0.0;
        }
        let (mut filled, mut tries) = (0, 0);
        while filled < hardware && tries < tracks {
            let channel = &mut self.channels[self.next_channel];
            if channel.active {
                let freq = if channel.arp {
                    let freq = channel.freq[channel.arp_counter];
                    channel.arp_counter = (channel.arp_counter + 1) % 3;
                    freq
                } else {
                    channel.freq[0]
                };
                self.outputs[filled].step = 1.0 / (rate / freq as f32);
                filled += 1;
            }
            self.next_channel = (self.next_channel + 1) % tracks;
            tries += 1;
        }
    }

    /// Reads the row's cells, then moves to the next row.
    fn row(&mut self) {
        let mut pattern_jump = self.order + 1;
        let mut row_jump = 0;
        let mut jump = false;
        let pattern = usize::from(self.song.order()[self.order]);
        for track in 0..usize::from(self.song.tracks()) {
            let cell = u32::from(self.song.cell(pattern, self.row, track));
            let mut note = (cell >> 9) & 127;
            let effect = (cell >> 6) & 7;
            let data = cell & 63;
            let (data1, data2) = ((cell >> 3) & 7, cell & 7);
            let channel = &mut self.channels[track];
            // Effects are off unless set again on every row.
            channel.portamento = 0;
            channel.arp = false;
            channel.vibrato = false;
            let old_hz = channel.freq[0];
            if note == 127 {
                channel.active = false;
                channel.freq = [0; 3];
                channel.last_note = 0;
            } else if note != 0 {
                channel.active = true;
                channel.freq = [note_hz(note as i32 * 8); 3];
                channel.last_note = note;
                channel.vibrato_index = 0;
            } else {
                note = channel.last_note;
            }
            let note = note as i32;
            match effect {
                0 => {
                    channel.freq[1] = note_hz((note + data1 as i32) * 8);
                    channel.freq[2] = note_hz((note + data2 as i32) * 8);
                    channel.arp = data1 != 0 || data2 != 0;
                }
                1 => channel.portamento = data as i32,
                2 => channel.portamento = -(data as i32),
                3 => {
                    channel.portamento_to_note = note_hz(note * 8);
                    if old_hz != channel.portamento_to_note {
                        channel.freq[0] = old_hz;
                        channel.portamento = data as i32;
                        if old_hz > channel.portamento_to_note {
                            channel.portamento = -channel.portamento;
                        }
                    } else {
                        channel.portamento_to_note = 0;
                    }
                }
                4 => {
                    channel.vibrato = true;
                    if data2 != 0 {
                        channel.vibrato_depth = data2 as i32;
                    }
                    if data1 != 0 {
                        channel.vibrato_speed = data1 as i32;
                    }
                }
                5 => {
                    pattern_jump = data as usize;
                    jump = true;
                }
                6 => {
                    row_jump = data as usize;
                    jump = true;
                }
                _ => self.tempo = data as i32,
            }
        }
        self.row += 1;
        if jump {
            self.row = row_jump;
            self.order = pattern_jump;
        }
        if self.row == ROWS {
            self.row = 0;
            self.order += 1;
            if self.order < 256 && self.song.order()[self.order] == 0xff {
                self.order = 0;
            }
        }
        // Past the order's last entry, start again (reference patch 0079).
        if self.order >= 256 {
            self.order = 0;
        }
    }
}

impl GeneratorInstance for MonotoneInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        _: usize,
        context: &GeneratorContext,
    ) -> usize {
        let rate = context.sample_rate;
        // At least a sample a tick (reference patch 0081; SoLoud divided
        // by 0 below 60 Hz).
        let per_tick = c_int(f64::from((rate / 60.0).floor())).max(1);
        let hardware = self.live.hardware_channels.load(Ordering::Relaxed) as usize;
        let wave = self.live.waveform.load(Ordering::Relaxed);
        let enabled = hardware.min(usize::from(self.song.tracks()));
        for sample in &mut out[..frames] {
            if self.sample_count % per_tick == 0 {
                self.tick(rate, hardware);
            }
            let mut s = 0.0_f32;
            for output in &mut self.outputs[..enabled] {
                let pos = output.pos + output.step;
                output.pos = pos - c_int(f64::from(pos)) as f32;
                s += waveform::generate(wave, output.pos) * 0.5;
            }
            *sample = s;
            self.sample_count = self.sample_count.wrapping_add(1);
        }
        frames
    }

    fn has_ended(&self) -> bool {
        false
    }
}
