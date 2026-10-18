// Ported from SoLoud src/audiosource/ay/chipplayer.cpp (zlib/libpng,
// Copyright (c) 2020 Jari Komppa), around the Unreal Speccy emulator by
// SMT (public domain); altered for SoLouder.

//! Plays a dump's operations on two chips into the shared ring, a frame of
//! about half the ring at a time (`ChipPlayer`).

use crate::chipdump::ChipDump;

use super::chip::Chip;
use super::render::{RING_SIZE, Ring};

/// The output rate.
const SAMPLE_RATE: u32 = 44100;
/// Levels are rendered at half volume and doubled when read.
const VOL_DIV: i32 = 2;

/// Ticks of a frame at `cpu_speed` ticks a second: half of what fits in
/// the ring, with 20 samples to spare. 0 below 11 Hz, which `Ay` refuses
/// (reference patch 0088).
pub(crate) fn ticks_per_frame(cpu_speed: u32) -> u32 {
    (i64::from(cpu_speed) * i64::from(RING_SIZE - 20) / i64::from(SAMPLE_RATE)) as u32 / 2
}

/// A voice's player (`ChipPlayer`). The ring is allocated once; `reset`
/// starts again in place.
#[derive(Debug)]
pub(crate) struct Player {
    ring: Ring,
    pub(crate) chip: Chip,
    pub(crate) chip2: Chip,
    cpu_speed: u32,
    chip_speed: u32,
    ym: bool,
    playtick: u32,
    /// Ticks of a wait left for the next frames (reference patch 0090).
    pub(crate) pending_ticks: u32,
    ticks_per_frame: u32,
}

impl Player {
    pub(crate) fn new(cpu_speed: u32, chip_speed: u32, ym: bool) -> Self {
        super::render::init_tables();
        Self {
            ring: Ring::new(),
            chip: Chip::new(cpu_speed, chip_speed, ym),
            chip2: Chip::new(cpu_speed, chip_speed, ym),
            cpu_speed,
            chip_speed,
            ym,
            playtick: 0,
            pending_ticks: 0,
            ticks_per_frame: ticks_per_frame(cpu_speed),
        }
    }

    /// Back to the state of a new player (reference patch 0092).
    pub(crate) fn reset(&mut self) {
        self.ring.reset();
        self.chip = Chip::new(self.cpu_speed, self.chip_speed, self.ym);
        self.chip2 = self.chip.clone();
        self.playtick = 0;
        self.pending_ticks = 0;
    }

    /// Samples rendered and not yet read (reference patch 0091).
    pub(crate) fn buffered(&self) -> u32 {
        self.ring
            .ready(&self.chip.render)
            .min(self.ring.ready(&self.chip2.render))
    }

    /// Reads `frames` frames into `out` (left, then right at `stride`;
    /// reference patch 0084), rendering frames of `dump` from `*pos` as
    /// needed. Returns how many it read: fewer once the dump has played.
    pub(crate) fn play(
        &mut self,
        out: &mut [f32],
        frames: usize,
        stride: usize,
        dump: Option<&ChipDump>,
        pos: &mut usize,
    ) -> usize {
        let length = dump.map_or(0, |dump| dump.length);
        let mut done = 0;
        while done < frames {
            let ready = self.buffered() as usize;
            let count = ready.min(frames - done);
            for i in 0..count {
                let at = (self.ring.read_position as usize + i) & (RING_SIZE as usize - 1);
                let sample = self.ring.samples[at];
                let l = VOL_DIV * (sample & 0xFFFF) as i32;
                let r = VOL_DIV * (sample >> 16) as i32;
                out[done + i] = l as f32 / 32767.0;
                out[stride + done + i] = r as f32 / 32767.0;
            }
            self.ring.samples_read(count as u32);
            done += count;
            if done == frames {
                break;
            }
            let Some(dump) = dump.filter(|_| *pos < length || self.pending_ticks != 0) else {
                break;
            };
            self.render_frame(dump, pos);
        }
        done
    }

    /// Plays operations for about a frame's ticks, then renders them.
    fn render_frame(&mut self, dump: &ChipDump, pos: &mut usize) {
        self.chip.start_frame();
        self.chip2.start_frame();
        let frame_tick = self.playtick;
        // The ring holds about twice a frame's ticks: a longer wait is
        // played over several frames (reference patch 0090).
        let limit = self.ticks_per_frame * 2;
        let step = self.pending_ticks.min(limit);
        self.playtick = self.playtick.wrapping_add(step);
        self.pending_ticks -= step;
        let used = |playtick: u32| playtick.wrapping_sub(frame_tick);
        while self.pending_ticks == 0
            && used(self.playtick) < self.ticks_per_frame
            && *pos < dump.length
        {
            let mut op;
            loop {
                op = dump.op(*pos);
                *pos += 2;
                if op & 0x8000 != 0 {
                    let mut wait = u32::from(op ^ 0x8000);
                    let room = limit - used(self.playtick);
                    if wait > room {
                        self.pending_ticks = wait - room;
                        wait = room;
                    }
                    self.playtick = self.playtick.wrapping_add(wait);
                }
                if op & 0x8000 == 0 || self.pending_ticks != 0 || *pos >= dump.length {
                    break;
                }
            }
            // Out of operations while waiting. A register write that is
            // the last operation is still played (reference patch 0089).
            if op & 0x8000 != 0 {
                break;
            }
            let reg = (op >> 8) as u8;
            let val = op as u8;
            let ts = used(self.playtick);
            let ring = &mut self.ring;
            if reg & 0x10 != 0 {
                self.chip2.select(reg & 0x0F);
                self.chip2.write(ring, ts, val);
            } else {
                self.chip.select(reg);
                self.chip.write(ring, ts, val);
            }
        }
        let ticks = used(self.playtick);
        self.chip.end_frame(&mut self.ring, ticks);
        self.chip2.end_frame(&mut self.ring, ticks);
    }
}
