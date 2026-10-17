//! Scenario operations on queues. Each mirrors the operation of the same
//! name in `reference/tests/parity_runner.cpp`.

use solouder_core::{Queue, QueueError, Sound, Stream, Synth};

use crate::ops::code;
use crate::run::{INVALID_PARAMETER, SO_NO_ERROR, State};
use crate::scenario::{ScenarioError, Step};

/// SoLoud's `OUT_OF_MEMORY`.
const OUT_OF_MEMORY: i32 = 5;

/// A sound or stream to queue.
enum Queued {
    Sound(Sound),
    Stream(Stream),
    Synth(Synth),
}

impl State {
    fn sound_named(&self, step: &Step, key: &str) -> Result<Sound, ScenarioError> {
        let name = step.text(key)?;
        self.sounds
            .get(name)
            .cloned()
            .ok_or_else(|| step.error(format!("no sound named `{name}`")))
    }

    /// A sound, else a stream, else a generator: what a queue can hold.
    fn queued_named(&self, step: &Step, key: &str) -> Result<Queued, ScenarioError> {
        let name = step.text(key)?;
        match (
            self.sounds.get(name),
            self.streams.get(name),
            self.synths.get(name),
        ) {
            (None, Some(stream), _) => Ok(Queued::Stream(stream.clone())),
            (None, None, Some(synth)) => Ok(Queued::Synth(synth.clone())),
            _ => self.sound_named(step, key).map(Queued::Sound),
        }
    }

    fn queue_named(&self, step: &Step, key: &str) -> Result<Queue, ScenarioError> {
        let name = step.text(key)?;
        self.queues
            .get(name)
            .cloned()
            .ok_or_else(|| step.error(format!("no queue named `{name}`")))
    }

    /// A new queue. With `channels` or `rate`, `setParams` (defaults 1 and
    /// 44100); with `from`, then `setParamsFromAudioSource`; then `setVolume`.
    /// Replacing a name stops the old queue's voices, as deleting the old
    /// `Queue` does in C++.
    pub(crate) fn queue(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["name", "channels", "rate", "from", "volume"])?;
        self.mixer(step)?;
        let name = step.text("name")?.to_owned();
        let mut queue = Queue::new();
        let mut result = SO_NO_ERROR;
        if step.get("channels").is_some() || step.get("rate").is_some() {
            let rate = step.number_or("rate", 44100.0)?;
            result = code(queue.set_params(rate, step.value_or("channels", 1)?));
        }
        if step.get("from").is_some() {
            let sound = self.sound_named(step, "from")?;
            queue.set_params_from(&sound);
        }
        queue.set_volume(step.number_or("volume", 1.0)?);
        if let Some(old) = self.queues.insert(name.clone(), queue) {
            self.mixer(step)?.stop_sound(&old);
        }
        self.log(format!("queue {name} {result}"));
        Ok(())
    }

    /// `Queue::play`.
    pub(crate) fn queue_add(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["queue", "sound"])?;
        let name = step.text("queue")?.to_owned();
        let queue = self.queue_named(step, "queue")?;
        let sound = self.queued_named(step, "sound")?;
        let mixer = self.mixer(step)?;
        let played = match &sound {
            Queued::Sound(sound) => mixer.queue_play(&queue, sound),
            Queued::Stream(stream) => mixer.queue_play(&queue, stream),
            Queued::Synth(synth) => mixer.queue_play(&queue, synth),
        };
        let result = match played {
            Ok(()) => SO_NO_ERROR,
            Err(QueueError::Full) => OUT_OF_MEMORY,
            Err(_) => INVALID_PARAMETER,
        };
        self.log(format!("queue_add {name} {result}"));
        Ok(())
    }

    /// Logs `getQueueCount`, and with `sound` `isCurrentlyPlaying`.
    pub(crate) fn queue_query(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["queue", "sound"])?;
        let name = step.text("queue")?.to_owned();
        let queue = self.queue_named(step, "queue")?;
        let sound = step
            .get("sound")
            .map(|_| self.queued_named(step, "sound"))
            .transpose()?;
        let mixer = self.mixer(step)?;
        let mut line = format!("queue_query {name} count={}", mixer.queue_count(&queue));
        if let Some(sound) = sound {
            let current = match &sound {
                Queued::Sound(sound) => mixer.queue_is_current(&queue, sound),
                Queued::Stream(stream) => mixer.queue_is_current(&queue, stream),
                Queued::Synth(synth) => mixer.queue_is_current(&queue, synth),
            };
            line.push_str(&format!(" current={}", u8::from(current)));
        }
        self.log(line);
        Ok(())
    }
}
