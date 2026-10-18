//! Scenario operations that play sounds, buses and queues, and on buses. Each
//! mirrors the operation of the same name in
//! `reference/tests/parity_runner.cpp`.

use solouder_core::{Bus, Handle, Mixer, PlayOptions, Playable, Queue, Resampler, Sound, Stream};

use crate::ops::{code, hex32};
use crate::run::State;
use crate::scenario::{ScenarioError, Step};

/// A sound, stream, bus or queue, by scenario name.
#[derive(Clone)]
pub(crate) enum Source {
    Sound(Sound),
    Stream(Stream),
    Bus(Bus),
    Queue(Queue),
}

/// How `play` starts a voice.
struct Start<'a> {
    through: Option<&'a Bus>,
    clock: Option<f64>,
    background: bool,
    options: PlayOptions,
}

fn start(mixer: &mut Mixer, source: &impl Playable, how: Start<'_>) -> Handle {
    let Start {
        through,
        clock,
        background,
        options,
    } = how;
    match (through, clock) {
        _ if background => mixer.play_background(source, options.volume, options.paused),
        (Some(bus), Some(time)) => mixer.bus_play_clocked(bus, time, source, options),
        (Some(bus), None) => mixer.bus_play(bus, source, options),
        (None, Some(time)) => mixer.play_clocked(time, source, options),
        (None, None) => mixer.play_with(source, options),
    }
}

/// `Bus::setResampler` ignores values past `RESAMPLER_CATMULLROM`.
pub(crate) fn resampler(value: u32) -> Option<Resampler> {
    match value {
        0 => Some(Resampler::Point),
        1 => Some(Resampler::Linear),
        2 => Some(Resampler::CatmullRom),
        _ => None,
    }
}

impl State {
    pub(crate) fn source(&self, step: &Step, key: &str) -> Result<Source, ScenarioError> {
        let name = step.text(key)?;
        if let Some(sound) = self.sounds.get(name) {
            return Ok(Source::Sound(sound.clone()));
        }
        if let Some(stream) = self.streams.get(name) {
            return Ok(Source::Stream(stream.clone()));
        }
        if let Some(bus) = self.buses.get(name) {
            return Ok(Source::Bus(bus.clone()));
        }
        self.queues
            .get(name)
            .map(|queue| Source::Queue(queue.clone()))
            .ok_or_else(|| step.error(format!("no sound, stream, bus or queue named `{name}`")))
    }

    pub(crate) fn bus_named(&self, step: &Step, key: &str) -> Result<Bus, ScenarioError> {
        let name = step.text(key)?;
        self.buses
            .get(name)
            .cloned()
            .ok_or_else(|| step.error(format!("no bus named `{name}`")))
    }

    pub(crate) fn handle_named(&self, step: &Step, key: &str) -> Result<Handle, ScenarioError> {
        let name = step.text(key)?;
        self.handles
            .get(name)
            .copied()
            .ok_or_else(|| step.error(format!("no handle named `{name}`")))
    }

    pub(crate) fn play(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&[
            "sound",
            "as",
            "volume",
            "pan",
            "paused",
            "clock",
            "background",
            "bus",
            "via",
        ])?;
        let present = |key: &str| step.get(key).is_some();
        if present("clock") && present("paused") {
            return Err(step.error("`clock` and `paused` cannot be combined"));
        }
        if present("bus") && present("via") {
            return Err(step.error("`bus` and `via` cannot be combined"));
        }
        let background = step.flag_or("background", false)?;
        if present("background") && ["pan", "clock", "bus", "via"].iter().any(|k| present(k)) {
            return Err(
                step.error("`background` cannot be combined with `pan`, `clock`, `bus` or `via`")
            );
        }
        let clock = step
            .get("clock")
            .map(|_| step.seconds_or("clock", 0.0))
            .transpose()?;
        let source = self.source(step, "sound")?;
        let through = step
            .get("bus")
            .map(|_| self.bus_named(step, "bus"))
            .transpose()?;
        let via = step
            .get("via")
            .map(|_| self.handle_named(step, "via"))
            .transpose()?;
        let name = step.text("as")?.to_owned();
        // `Soloud::play` treats a negative volume as "use the source's";
        // `Bus::play` defaults to 1.
        let default = if through.is_some() { 1.0 } else { -1.0 };
        let volume = step.number_or("volume", default)?;
        let how = Start {
            through: through.as_ref(),
            clock,
            background,
            options: PlayOptions {
                volume: (volume >= 0.0).then_some(volume),
                pan: step.number_or("pan", 0.0)?,
                paused: step.flag_or("paused", false)?,
                bus: via.unwrap_or(Handle::INVALID),
                ..PlayOptions::default()
            },
        };
        let mixer = self.mixer(step)?;
        let handle = match &source {
            Source::Sound(sound) => start(mixer, sound, how),
            Source::Stream(stream) => start(mixer, stream, how),
            Source::Bus(bus) => start(mixer, bus, how),
            Source::Queue(queue) => start(mixer, queue, how),
        };
        let sound_name = step.text("sound")?;
        self.played
            .entry(sound_name.to_owned())
            .or_default()
            .push(handle);
        self.handles.insert(name.clone(), handle);
        self.log(format!("play {name} {}", handle.raw()));
        Ok(())
    }

    /// A new bus. Replacing a name stops the old bus's voices, as deleting
    /// the old `Bus` does in C++.
    pub(crate) fn bus(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["name", "channels", "resampler", "visualization", "volume"])?;
        self.mixer(step)?;
        let name = step.text("name")?.to_owned();
        let mut bus = Bus::new();
        let mut result = Ok(());
        if step.get("channels").is_some() {
            result = bus.set_channels(step.value("channels")?);
        }
        if step.get("resampler").is_some()
            && let Some(resampler) = resampler(step.value("resampler")?)
        {
            bus.set_resampler(resampler);
        }
        bus.set_visualization(step.flag_or("visualization", false)?);
        bus.set_volume(step.number_or("volume", 1.0)?);
        if let Some(old) = self.buses.insert(name.clone(), bus) {
            self.mixer(step)?.stop_sound(&old);
        }
        self.log(format!("bus {name} {}", code(result)));
        Ok(())
    }

    pub(crate) fn annex(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["bus", "h"])?;
        let bus = self.bus_named(step, "bus")?;
        let handle = self.handle(step)?;
        self.mixer(step)?.annex_sound(&bus, handle);
        Ok(())
    }

    /// Logs the bus's voice count (`getActiveVoiceCount`, which looks up the
    /// bus handle), `countAudioSource` and the approximate volume of
    /// channels 0 to 9.
    pub(crate) fn bus_query(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["bus"])?;
        let name = step.text("bus")?.to_owned();
        let bus = self.bus_named(step, "bus")?;
        let mixer = self.mixer(step)?;
        let mut line = format!(
            "bus_query {name} voices={} count={} volumes=",
            mixer.bus_voice_count(&bus),
            mixer.count_sound(&bus)
        );
        let volumes: Vec<String> = (0..10)
            .map(|c| hex32(mixer.bus_approximate_volume(&bus, c)))
            .collect();
        line.push_str(&volumes.join(","));
        self.log(line);
        Ok(())
    }

    /// Appends `getWave`, or with `fft` `calcFFT`, to the samples; logs
    /// `none` instead when the bus is not playing.
    pub(crate) fn bus_wave(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["bus", "fft"])?;
        let name = step.text("bus")?.to_owned();
        let bus = self.bus_named(step, "bus")?;
        let fft = step.flag_or("fft", false)?;
        let mixer = self.mixer(step)?;
        let data = if fft {
            mixer.bus_fft(&bus)
        } else {
            mixer.bus_wave(&bus)
        };
        match data {
            Some(data) => {
                self.rendered.samples.extend_from_slice(&data);
                self.log(format!("bus_wave {name}"));
            }
            None => self.log(format!("bus_wave {name} none")),
        }
        Ok(())
    }

    /// `Bus::setFilter`; without `filter` the slot is cleared.
    pub(crate) fn bus_filter(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["bus", "slot", "filter"])?;
        let name = step.text("bus")?.to_owned();
        self.bus_named(step, "bus")?;
        let slot: u32 = step.value("slot")?;
        let filter = self.named_filter(step, "filter")?;
        let Some(mut bus) = self.buses.remove(&name) else {
            return Ok(());
        };
        self.mixer(step)?.set_bus_filter(&mut bus, slot, filter);
        self.buses.insert(name, bus);
        Ok(())
    }

    /// Bus settings, applied in order: channels, resampler, visualization.
    pub(crate) fn bus_set(&mut self, step: &Step) -> Result<(), ScenarioError> {
        step.check_keys(&["bus", "channels", "resampler", "visualization"])?;
        let name = step.text("bus")?.to_owned();
        let mut bus = self.bus_named(step, "bus")?;
        let channels: Option<u32> = step
            .get("channels")
            .map(|_| step.value("channels"))
            .transpose()?;
        let resampler_value: Option<u32> = step
            .get("resampler")
            .map(|_| step.value("resampler"))
            .transpose()?;
        let visualization = step
            .get("visualization")
            .map(|_| step.flag_or("visualization", false))
            .transpose()?;
        let mixer = self.mixer(step)?;
        let mut result = None;
        if let Some(channels) = channels {
            result = Some(code(bus.set_channels(channels)));
        }
        if let Some(resampler) = resampler_value.and_then(resampler) {
            mixer.set_bus_resampler(&mut bus, resampler);
        }
        if let Some(enable) = visualization {
            mixer.set_bus_visualization(&mut bus, enable);
        }
        self.buses.insert(name.clone(), bus);
        if let Some(result) = result {
            self.log(format!("bus_set {name} channels {result}"));
        }
        Ok(())
    }
}
