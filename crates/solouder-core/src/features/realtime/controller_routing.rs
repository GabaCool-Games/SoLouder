//! Controller requests for filters, buses and queues. Filter instances and
//! queued sounds are built here; the instances they replace come back to be
//! freed on this thread.

use std::sync::Arc;

use super::VoiceId;
use super::command::{Command, ParamChange};
use super::controller::{Controller, ControllerError};
use crate::dsp::Resampler;
use crate::features::filters::{FILTERS_PER_STREAM, Filter};
use crate::features::playback::{BUS_RATE, Bus, Queue, Queueable};

/// Which filters a filter parameter request changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterTarget {
    /// The global filters.
    Global,
    /// The voice's filters.
    Voice(VoiceId),
}

impl Controller {
    /// Sets or clears global filter `slot` (0 to 7). The instance is made
    /// here, with the filter's current settings.
    pub fn set_global_filter(
        &mut self,
        slot: u32,
        filter: Option<&dyn Filter>,
    ) -> Result<(), ControllerError> {
        let instance = filter.map(|filter| {
            let mut instance = filter.create_instance();
            let config = self.config;
            instance.prepare(config.channels as usize, config.sample_rate as f32);
            instance
        });
        self.send_retiring(Command::GlobalFilter { slot, instance })
    }

    /// Sets a filter parameter, cancelling its fade.
    pub fn set_filter_parameter(
        &mut self,
        target: FilterTarget,
        slot: u32,
        param: u32,
        value: f32,
    ) -> Result<(), ControllerError> {
        self.filter_parameter(target, slot, param, ParamChange::Set(value))
    }

    /// Fades a filter parameter to `to` over `seconds`.
    pub fn fade_filter_parameter(
        &mut self,
        target: FilterTarget,
        slot: u32,
        param: u32,
        to: f32,
        seconds: f64,
    ) -> Result<(), ControllerError> {
        let change = ParamChange::Fade { to, seconds };
        self.filter_parameter(target, slot, param, change)
    }

    /// Oscillates a filter parameter between `from` and `to` with a period
    /// of `seconds`.
    pub fn oscillate_filter_parameter(
        &mut self,
        target: FilterTarget,
        slot: u32,
        param: u32,
        from: f32,
        to: f32,
        seconds: f64,
    ) -> Result<(), ControllerError> {
        let change = ParamChange::Oscillate { from, to, seconds };
        self.filter_parameter(target, slot, param, change)
    }

    fn filter_parameter(
        &mut self,
        target: FilterTarget,
        slot: u32,
        param: u32,
        change: ParamChange,
    ) -> Result<(), ControllerError> {
        let voice = match target {
            FilterTarget::Global => None,
            FilterTarget::Voice(id) => Some(id),
        };
        self.send(Command::FilterParameter {
            voice,
            slot,
            param,
            change,
        })
    }

    /// Sets or clears filter `slot` of `bus` for later plays and, if the bus
    /// is playing, replaces its voice's filter (`Bus::setFilter`). The
    /// instance is prepared for the bus voice this controller started last,
    /// whose channels and speed can differ from `bus`'s settings now.
    pub fn set_bus_filter(
        &mut self,
        bus: &mut Bus,
        slot: u32,
        filter: Option<Arc<dyn Filter>>,
    ) -> Result<(), ControllerError> {
        let (channels, rate) = self
            .buses
            .of(bus.id())
            .unwrap_or((bus.channels() as usize, BUS_RATE));
        let instance = filter.as_ref().map(|filter| {
            let mut instance = filter.create_instance();
            instance.prepare(channels, rate);
            instance
        });
        if (slot as usize) < FILTERS_PER_STREAM {
            self.send_retiring(Command::BusFilter {
                bus: bus.id(),
                slot,
                instance,
            })?;
        }
        self.spatial
            .set_bus_filter(bus.id(), slot as usize, filter.clone());
        bus.set_filter(slot, filter);
        Ok(())
    }

    /// Sets how `bus` resamples its voices, including a playing bus.
    pub fn set_bus_resampler(
        &mut self,
        bus: &mut Bus,
        resampler: Resampler,
    ) -> Result<(), ControllerError> {
        self.send(Command::BusResampler(bus.id(), resampler))?;
        bus.set_resampler(resampler);
        Ok(())
    }

    /// Sets whether `bus` records visualization data, including a playing
    /// bus.
    pub fn set_bus_visualization(
        &mut self,
        bus: &mut Bus,
        enable: bool,
    ) -> Result<(), ControllerError> {
        self.send(Command::BusVisualization(bus.id(), enable))?;
        bus.set_visualization(enable);
        Ok(())
    }

    /// Moves a voice to play through `bus` (`Bus::annexSound`).
    pub fn annex_sound(&mut self, bus: &Bus, voice: VoiceId) -> Result<(), ControllerError> {
        self.send(Command::Annex {
            bus: bus.id(),
            voice,
        })
    }

    /// Queues `sound` (a [`Sound`](crate::Sound) or a
    /// [`Stream`](crate::Stream), which the
    /// [`StreamWorker`](super::StreamWorker) decodes) on `queue`
    /// (`Queue::play`). The queue must have been played through this
    /// controller. A sound the renderer cannot queue, because the queue is
    /// not playing or holds [`QUEUE_MAX`](crate::QUEUE_MAX) sounds, is
    /// dropped and counted in
    /// [`Status::rejected_queue_sounds`](super::Status::rejected_queue_sounds).
    pub fn queue_play(
        &mut self,
        queue: &Queue,
        sound: &impl Queueable,
    ) -> Result<(), ControllerError> {
        if sound.stream().is_some() {
            self.stream_room()?;
        }
        let underflow = Arc::clone(&self.underflow);
        let (item, feed) = sound.queued_fed(self.stream_chunks, underflow);
        self.send_retiring(Command::QueueAdd {
            queue: queue.id(),
            item,
        })?;
        if let Some(feed) = feed {
            // Cannot fail: checked above, and only this thread pushes.
            let _ = self.feeds.push(feed);
        }
        Ok(())
    }
}
