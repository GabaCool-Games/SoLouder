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
/// One record: the wave, the channel volumes, and the [`SoundId`] of the
/// bus it holds (low and high words), 0 when that bus was not playing. A
/// slot taken over by another bus shows the old bus's data until the
/// renderer has the new one; the ID tells them apart.
const RECORD: usize = WAVE_LEN + MAX + 2;
const ID: usize = WAVE_LEN + MAX;
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
        store(master, Some((wave, volumes)), 0);
        for (record, bus) in buses.as_chunks::<RECORD>().0.iter().zip(&self.buses) {
            let Some(bus) = bus else { break };
            let data = mixer.bus_voice_of(*bus);
            store(
                record,
                data.map(|v| (&v.wave, &v.channel_volume)),
                bus.get(),
            );
        }
        let back = self.back as u8 | FRESH;
        self.back = usize::from(self.buffers.exchange.swap(back, Ordering::AcqRel) & !FRESH);
    }
}

fn store(record: &[AtomicU32], data: Option<(&[f32; WAVE_LEN], &[f32; MAX])>, id: u64) {
    let relaxed = Ordering::Relaxed;
    let id = match data {
        Some((wave, volumes)) => {
            for (cell, value) in record.iter().zip(wave.iter().chain(volumes)) {
                cell.store(value.to_bits(), relaxed);
            }
            id
        }
        None => 0,
    };
    record[ID].store(id as u32, relaxed);
    record[ID + 1].store((id >> 32) as u32, relaxed);
}

fn stored_id(record: &[AtomicU32]) -> u64 {
    let word = |i: usize| u64::from(record[i].load(Ordering::Relaxed));
    word(ID) | word(ID + 1) << 32
}

/// The controller's side.
#[derive(Debug)]
pub(super) struct VisualReader {
    buffers: Arc<Buffers>,
    front: usize,
    /// The bus of each slot handed out, and whether the slot is released:
    /// its bus stopped recording, so another bus may take the slot over.
    buses: Vec<(SoundId, bool)>,
    capacity: usize,
}

impl VisualReader {
    /// A command giving `bus` a slot, if it needs one: a free slot, else
    /// a released one; fails when every slot is held. Call
    /// [`VisualReader::watched`] once it is sent.
    pub fn watch(&self, bus: SoundId) -> Result<Option<Command>, ControllerError> {
        if self.slot_of(bus).is_some() {
            return Ok(None);
        }
        let slot = if self.buses.len() < self.capacity {
            self.buses.len()
        } else {
            let released = self.buses.iter().position(|&(_, released)| released);
            released.ok_or(ControllerError::TooManyVisualizedBuses)?
        };
        Ok(Some(Command::WatchBus { slot, bus }))
    }

    /// Records the slot a command from [`VisualReader::watch`] gave its
    /// bus. Commands reach the renderer in order, so taking a released
    /// slot over needs no acknowledgement: the record's ID tells the old
    /// bus's data from the new one's.
    pub fn watched(&mut self, command: &Command) {
        let &Command::WatchBus { slot, bus } = command else {
            return;
        };
        match self.buses.get_mut(slot) {
            Some(entry) => *entry = (bus, false),
            None => self.buses.push((bus, false)),
        }
    }

    /// Holds `bus`'s slot, or releases it. A released slot keeps its bus's
    /// last data until another bus takes it over.
    pub fn hold(&mut self, bus: SoundId, held: bool) {
        if let Some(slot) = self.slot_of(bus) {
            self.buses[slot].1 = !held;
        }
    }

    fn slot_of(&self, bus: SoundId) -> Option<usize> {
        self.buses.iter().position(|&(b, _)| b == bus)
    }

    /// The mix's record in the latest copy.
    pub fn master(&mut self) -> ([f32; WAVE_LEN], [f32; MAX]) {
        load(self.latest(0))
    }

    /// The watched bus's record in the latest copy; `None` for a bus with
    /// no slot or not playing at the last render that published.
    pub fn bus(&mut self, bus: SoundId) -> Option<([f32; WAVE_LEN], [f32; MAX])> {
        let index = 1 + self.slot_of(bus)?;
        let record = self.latest(index);
        (stored_id(record) == bus.get()).then(|| load(record))
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
        reader.watched(&Command::WatchBus {
            slot,
            bus: bus.id(),
        });
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
