//! Visualization data from the audio thread: after each render the
//! renderer copies the mix's wave and channel volumes, and those of the
//! buses the controller watches, into a triple buffer of atomics. Neither
//! side waits or allocates; the controller reads the latest complete copy.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use super::ControllerError;
use super::command::Command;
use crate::contracts::MAX_CHANNELS;
use crate::features::playback::{Mixer, SoundId, WAVE_LEN};

const MAX: usize = MAX_CHANNELS as usize;
/// One record: the wave, the channel volumes, and whether it is current
/// (1) or its bus was not playing (0).
const RECORD: usize = WAVE_LEN + MAX + 1;
/// Set in the exchange word when the middle buffer holds a copy the reader
/// has not taken.
const FRESH: u8 = 4;

/// Three copies of every record: the mix's first, then one per watched
/// bus. The renderer owns one copy and the controller another; the
/// exchange word holds the index of the third, swapped as each side
/// finishes with its own.
#[derive(Debug)]
struct Buffers {
    copies: [Box<[AtomicU32]>; 3],
    exchange: AtomicU8,
}

/// Allocates the buffers for the mix and up to `buses` watched buses.
pub(super) fn visual_channel(buses: usize) -> (VisualWriter, VisualReader) {
    let len = RECORD * (1 + buses);
    let copy = || (0..len).map(|_| AtomicU32::new(0)).collect();
    let buffers = Arc::new(Buffers {
        copies: [copy(), copy(), copy()],
        exchange: AtomicU8::new(1),
    });
    let writer = VisualWriter {
        buffers: Arc::clone(&buffers),
        back: 0,
        buses: vec![None; buses].into_boxed_slice(),
    };
    let reader = VisualReader {
        buffers,
        front: 2,
        buses: Vec::with_capacity(buses),
        capacity: buses,
    };
    (writer, reader)
}

/// The renderer's side.
#[derive(Debug)]
pub(super) struct VisualWriter {
    buffers: Arc<Buffers>,
    back: usize,
    /// The bus each slot watches.
    buses: Box<[Option<SoundId>]>,
}

impl VisualWriter {
    /// Starts copying `bus`'s data into record `slot + 1`.
    pub fn watch(&mut self, slot: usize, bus: SoundId) {
        if let Some(entry) = self.buses.get_mut(slot) {
            *entry = Some(bus);
        }
    }

    /// Copies the mix's data and every watched bus's, if anything records
    /// it; otherwise the last copy stands.
    pub fn publish(&mut self, mixer: &Mixer) {
        if !mixer.visualization() && self.buses.first().is_none_or(Option::is_none) {
            return;
        }
        let copy = &self.buffers.copies[self.back];
        let (master, buses) = copy.split_at(RECORD);
        let (wave, volumes) = mixer.visualization_parts();
        store(master, Some((wave, volumes)));
        for (record, bus) in buses.as_chunks::<RECORD>().0.iter().zip(&self.buses) {
            let Some(bus) = bus else { break };
            let data = mixer.bus_voice_of(*bus);
            store(record, data.map(|v| (&v.wave, &v.channel_volume)));
        }
        let back = self.back as u8 | FRESH;
        self.back = usize::from(self.buffers.exchange.swap(back, Ordering::AcqRel) & !FRESH);
    }
}

fn store(record: &[AtomicU32], data: Option<(&[f32; WAVE_LEN], &[f32; MAX])>) {
    let relaxed = Ordering::Relaxed;
    let Some((wave, volumes)) = data else {
        record[RECORD - 1].store(0, relaxed);
        return;
    };
    for (cell, value) in record.iter().zip(wave.iter().chain(volumes)) {
        cell.store(value.to_bits(), relaxed);
    }
    record[RECORD - 1].store(1, relaxed);
}

/// The controller's side.
#[derive(Debug)]
pub(super) struct VisualReader {
    buffers: Arc<Buffers>,
    front: usize,
    /// The bus of each slot handed out.
    buses: Vec<SoundId>,
    capacity: usize,
}

impl VisualReader {
    /// A command giving `bus` a slot, if it needs one; fails when every
    /// slot is taken. Call [`VisualReader::watched`] once it is sent.
    pub fn watch(&self, bus: SoundId) -> Result<Option<Command>, ControllerError> {
        if self.buses.contains(&bus) {
            return Ok(None);
        }
        if self.buses.len() >= self.capacity {
            return Err(ControllerError::TooManyVisualizedBuses);
        }
        let slot = self.buses.len();
        Ok(Some(Command::WatchBus { slot, bus }))
    }

    /// Records the slot [`VisualReader::watch`] gave `bus`.
    pub fn watched(&mut self, bus: SoundId) {
        self.buses.push(bus);
    }

    /// The mix's record in the latest copy.
    pub fn master(&mut self) -> ([f32; WAVE_LEN], [f32; MAX]) {
        load(self.latest(0))
    }

    /// The watched bus's record in the latest copy; `None` for a bus with
    /// no slot or not playing at the last render that published.
    pub fn bus(&mut self, bus: SoundId) -> Option<([f32; WAVE_LEN], [f32; MAX])> {
        let index = 1 + self.buses.iter().position(|&b| b == bus)?;
        let record = self.latest(index);
        (record[RECORD - 1].load(Ordering::Relaxed) != 0).then(|| load(record))
    }

    /// Record `index` of the latest copy, taking a fresh one if the
    /// renderer has published since the last read.
    fn latest(&mut self, index: usize) -> &[AtomicU32] {
        let exchange = &self.buffers.exchange;
        if exchange.load(Ordering::Relaxed) & FRESH != 0 {
            self.front = usize::from(exchange.swap(self.front as u8, Ordering::AcqRel) & !FRESH);
        }
        &self.buffers.copies[self.front][index * RECORD..][..RECORD]
    }
}

fn load(record: &[AtomicU32]) -> ([f32; WAVE_LEN], [f32; MAX]) {
    let value = |i: usize| f32::from_bits(record[i].load(Ordering::Relaxed));
    (
        std::array::from_fn(value),
        std::array::from_fn(|i| value(WAVE_LEN + i)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bus, MixerConfig, Pcm, Sound};

    #[test]
    fn the_reader_sees_the_latest_copy_and_keeps_it() {
        let config = MixerConfig {
            visualization: true,
            ..MixerConfig::default()
        };
        let mut mixer = Mixer::new(config).unwrap();
        let pcm = Pcm::from_planar(vec![0.25; 44_100], 1, 44100.0).unwrap();
        mixer.play(&Sound::new(pcm));
        let (mut writer, mut reader) = visual_channel(1);
        let bus = Bus::new();
        assert_eq!(reader.bus(bus.id()), None);
        let Ok(Some(Command::WatchBus { slot, .. })) = reader.watch(bus.id()) else {
            panic!("a free slot");
        };
        reader.watched(bus.id());
        writer.watch(slot, bus.id());
        assert_eq!(
            reader.watch(Bus::new().id()).err(),
            Some(ControllerError::TooManyVisualizedBuses)
        );
        let mut out = vec![0.0; 1024];
        for round in 0..5 {
            mixer.set_global_volume(1.0 + round as f32);
            mixer.mix_interleaved(&mut out);
            writer.publish(&mixer);
            // Twice: a second read without a new copy keeps the last.
            for _ in 0..2 {
                let (wave, _) = reader.master();
                assert_eq!(wave, mixer.wave(), "round {round}");
            }
            // The bus is not playing.
            assert_eq!(reader.bus(bus.id()), None);
        }
    }
}
