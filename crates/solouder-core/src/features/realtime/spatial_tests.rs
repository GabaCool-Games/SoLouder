//! 3D through the controller renders what the offline mixer renders, bit for
//! bit: the same scripts run on a `Mixer` and on a controller and renderer.

use super::*;
use crate::spatial::{Attenuation, Attenuator, Collider, SpatialVoice};
use crate::{Bus, Handle, Mixer, MixerConfig, Pcm, PlayOptions, Sound};

/// The calls the scripts below make, on a mixer or on a controller and its
/// renderer.
pub(super) trait Engine {
    type Id: Copy;
    fn play_3d(&mut self, s: &Sound, at: [f32; 3], v: [f32; 3], o: PlayOptions) -> Self::Id;
    fn play_3d_clocked(&mut self, t: f64, s: &Sound, at: [f32; 3], o: PlayOptions) -> Self::Id;
    fn play_bus(&mut self, bus: &Bus);
    fn bus_play_3d(&mut self, bus: &Bus, s: &Sound, at: [f32; 3], v: [f32; 3]) -> Self::Id;
    fn bus_play_3d_clocked(&mut self, bus: &Bus, t: f64, s: &Sound, at: [f32; 3]) -> Self::Id;
    fn listener(&mut self, at: [f32; 3], facing: [f32; 3], up: [f32; 3], v: [f32; 3]);
    fn sound_speed(&mut self, speed: f32);
    fn speaker(&mut self, channel: u32, at: [f32; 3]);
    fn source(&mut self, id: Self::Id, at: [f32; 3], v: [f32; 3]);
    fn source_settings(&mut self, id: Self::Id, doppler: f32, min_max: (f32, f32));
    fn attenuation(&mut self, id: Self::Id, attenuation: Attenuation, rolloff: f32);
    fn volume(&mut self, id: Self::Id, volume: f32);
    fn pause(&mut self, id: Self::Id, paused: bool);
    fn speed(&mut self, id: Self::Id, speed: f32);
    fn update(&mut self);
    fn mix(&mut self, frames: usize) -> Vec<f32>;
}

impl Engine for Mixer {
    type Id = Handle;
    fn play_3d(&mut self, s: &Sound, at: [f32; 3], v: [f32; 3], o: PlayOptions) -> Handle {
        Mixer::play_3d(self, s, at, v, o)
    }
    fn play_3d_clocked(&mut self, t: f64, s: &Sound, at: [f32; 3], o: PlayOptions) -> Handle {
        Mixer::play_3d_clocked(self, t, s, at, STILL, o)
    }
    fn play_bus(&mut self, bus: &Bus) {
        self.play(bus);
    }
    fn bus_play_3d(&mut self, bus: &Bus, s: &Sound, at: [f32; 3], v: [f32; 3]) -> Handle {
        Mixer::bus_play_3d(self, bus, s, at, v, PlayOptions::default())
    }
    fn bus_play_3d_clocked(&mut self, bus: &Bus, t: f64, s: &Sound, at: [f32; 3]) -> Handle {
        Mixer::bus_play_3d_clocked(self, bus, t, s, at, STILL, PlayOptions::default())
    }
    fn listener(&mut self, at: [f32; 3], facing: [f32; 3], up: [f32; 3], v: [f32; 3]) {
        self.set_3d_listener_parameters(at, facing, up, v);
    }
    fn sound_speed(&mut self, speed: f32) {
        self.set_3d_sound_speed(speed).unwrap();
    }
    fn speaker(&mut self, channel: u32, at: [f32; 3]) {
        self.set_speaker_position(channel, at).unwrap();
    }
    fn source(&mut self, id: Handle, at: [f32; 3], v: [f32; 3]) {
        self.set_3d_source_parameters(id, at, v);
    }
    fn source_settings(&mut self, id: Handle, doppler: f32, (min, max): (f32, f32)) {
        self.set_3d_source_doppler_factor(id, doppler);
        self.set_3d_source_min_max_distance(id, min, max);
    }
    fn attenuation(&mut self, id: Handle, attenuation: Attenuation, rolloff: f32) {
        self.set_3d_source_attenuation(id, attenuation, rolloff);
    }
    fn volume(&mut self, id: Handle, volume: f32) {
        self.set_volume(id, volume);
    }
    fn pause(&mut self, id: Handle, paused: bool) {
        self.set_pause(id, paused);
    }
    fn speed(&mut self, id: Handle, speed: f32) {
        self.set_relative_play_speed(id, speed).unwrap();
    }
    fn update(&mut self) {
        self.update_3d_audio();
    }
    fn mix(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * self.config().channels as usize];
        self.mix_interleaved(&mut out);
        out
    }
}

/// A controller and its renderer; events are read after every render.
pub(super) struct Realtime(pub Controller, pub Renderer);

impl Engine for Realtime {
    type Id = VoiceId;
    fn play_3d(&mut self, s: &Sound, at: [f32; 3], v: [f32; 3], o: PlayOptions) -> VoiceId {
        self.0.play_3d(s, at, v, o).unwrap()
    }
    fn play_3d_clocked(&mut self, t: f64, s: &Sound, at: [f32; 3], o: PlayOptions) -> VoiceId {
        self.0.play_3d_clocked(t, s, at, STILL, o).unwrap()
    }
    fn play_bus(&mut self, bus: &Bus) {
        self.0.play(bus).unwrap();
    }
    fn bus_play_3d(&mut self, bus: &Bus, s: &Sound, at: [f32; 3], v: [f32; 3]) -> VoiceId {
        let options = PlayOptions::default();
        self.0.bus_play_3d(bus, s, at, v, options).unwrap()
    }
    fn bus_play_3d_clocked(&mut self, bus: &Bus, t: f64, s: &Sound, at: [f32; 3]) -> VoiceId {
        let options = PlayOptions::default();
        self.0
            .bus_play_3d_clocked(bus, t, s, at, STILL, options)
            .unwrap()
    }
    fn listener(&mut self, at: [f32; 3], facing: [f32; 3], up: [f32; 3], v: [f32; 3]) {
        self.0.set_3d_listener_parameters(at, facing, up, v);
    }
    fn sound_speed(&mut self, speed: f32) {
        self.0.set_3d_sound_speed(speed).unwrap();
    }
    fn speaker(&mut self, channel: u32, at: [f32; 3]) {
        self.0.set_speaker_position(channel, at).unwrap();
    }
    fn source(&mut self, id: VoiceId, at: [f32; 3], v: [f32; 3]) {
        self.0.set_3d_source_parameters(id, at, v);
    }
    fn source_settings(&mut self, id: VoiceId, doppler: f32, (min, max): (f32, f32)) {
        self.0.set_3d_source_doppler_factor(id, doppler);
        self.0.set_3d_source_min_max_distance(id, min, max);
    }
    fn attenuation(&mut self, id: VoiceId, attenuation: Attenuation, rolloff: f32) {
        self.0.set_3d_source_attenuation(id, attenuation, rolloff);
    }
    fn volume(&mut self, id: VoiceId, volume: f32) {
        self.0.set_volume(id, volume).unwrap();
    }
    fn pause(&mut self, id: VoiceId, paused: bool) {
        self.0.set_pause(id, paused).unwrap();
    }
    fn speed(&mut self, id: VoiceId, speed: f32) {
        self.0.set_relative_play_speed(id, speed).unwrap();
    }
    fn update(&mut self) {
        self.0.update_3d_audio().unwrap();
    }
    fn mix(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * self.1.config().channels as usize];
        self.1.render(&mut out);
        while self.0.poll_event().is_some() {}
        out
    }
}

pub(super) fn both(config: MixerConfig) -> (Mixer, Realtime) {
    let mixer = Mixer::new(config).unwrap();
    let (controller, renderer) = split(Mixer::new(config).unwrap(), RealtimeConfig::default());
    (mixer, Realtime(controller, renderer))
}

fn compare(config: MixerConfig, offline: Vec<f32>, realtime: Vec<f32>) {
    assert_eq!(offline.len(), realtime.len(), "{config:?}");
    assert!(offline.iter().any(|&s| s != 0.0), "the script is silent");
    for (i, (a, b)) in offline.iter().zip(&realtime).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "sample {i} differs ({config:?})");
    }
}

pub(super) fn tone(frames: usize, channels: u32, rate: f32, period: usize) -> Sound {
    let samples = (0..frames * channels as usize)
        .map(|i| (i % period) as f32 / period as f32 - 0.5)
        .collect();
    Sound::new(Pcm::from_planar(samples, channels, rate).unwrap())
}

pub(super) const STILL: [f32; 3] = [0.0; 3];
const AHEAD: [f32; 3] = [0.0, 0.0, -1.0];
const UP: [f32; 3] = [0.0, 1.0, 0.0];

/// Attenuation models, moving sources and listener, Doppler with play
/// speed, pauses, the speed of sound, and voices made inaudible (ticking,
/// killed at play and at update) or held at the speed of sound.
fn moving<E: Engine>(e: &mut E) -> Vec<f32> {
    let mut near = tone(40000, 1, 44100.0, 61);
    near.set_3d_attenuation(Attenuation::InverseDistance, 1.0);
    near.set_3d_min_max_distance(1.0, 200.0);
    let mut far = tone(40000, 2, 22050.0, 37);
    far.set_3d_attenuation(Attenuation::ExponentialDistance, 0.7);
    far.set_inaudible_behavior(true, false);
    let mut quiet = tone(20000, 1, 44100.0, 13);
    quiet.set_3d_attenuation(Attenuation::LinearDistance, 1.0);
    quiet.set_3d_min_max_distance(1.0, 50.0);
    quiet.set_inaudible_behavior(false, true);
    let options = PlayOptions::default();
    let paused = PlayOptions {
        paused: true,
        ..options
    };
    let mut out = Vec::new();
    let a = e.play_3d(&near, [3.0, 0.0, -4.0], [0.0, 0.0, 40.0], options);
    let b = e.play_3d(&far, [-6.0, 1.0, 2.0], STILL, options);
    let c = e.play_3d(&quiet, [0.0, 0.0, -20.0], STILL, paused);
    e.play_3d(&quiet, [0.0, 0.0, -80.0], STILL, options);
    out.extend(e.mix(700));
    e.listener([1.0, 0.0, 1.0], [1.0, 0.0, -1.0], UP, [0.0, 0.0, -30.0]);
    e.source(a, [-2.0, 0.5, -3.0], [10.0, 0.0, 0.0]);
    e.source_settings(b, 2.5, (2.0, 900.0));
    e.update();
    e.pause(c, false);
    out.extend(e.mix(900));
    e.volume(b, 0.4);
    e.speed(a, 1.3);
    e.attenuation(b, Attenuation::InverseDistance, 3.0);
    e.source(c, [0.0, 0.0, -60.0], STILL);
    e.sound_speed(170.0);
    e.update();
    out.extend(e.mix(1100));
    e.source(b, [-6000.0, 0.0, 0.0], STILL);
    e.update();
    out.extend(e.mix(800));
    e.source(b, [1.0, 0.0, -2.0], [0.0, 0.0, 400.0]);
    e.update();
    out.extend(e.mix(600));
    e.source(b, [1.0, 0.0, -2.0], STILL);
    e.update();
    out.extend(e.mix(600));
    out
}

/// Distance delay, clocked plays, and plays through a bus, one not playing.
fn delays_and_buses<E: Engine>(e: &mut E) -> Vec<f32> {
    let mut delayed = tone(30000, 1, 44100.0, 29);
    delayed.set_3d_distance_delay(true);
    delayed.set_3d_attenuation(Attenuation::InverseDistance, 0.5);
    let plain = tone(30000, 2, 44100.0, 47);
    let bus = Bus::new();
    let idle = Bus::new();
    let options = PlayOptions::default();
    let mut out = Vec::new();
    e.sound_speed(3433.0);
    e.listener([5.0, 0.0, 0.0], AHEAD, UP, STILL);
    e.play_3d(&delayed, [5.0, 0.0, -34.33], STILL, options);
    e.play_3d_clocked(1.0, &delayed, [40.0, 0.0, -10.0], options);
    e.play_3d_clocked(1.004, &plain, [0.0, 2.0, 0.0], options);
    out.extend(e.mix(1500));
    e.play_bus(&bus);
    let v = e.bus_play_3d(&bus, &plain, [3.0, 0.0, -3.0], [0.0, 0.0, 20.0]);
    e.bus_play_3d_clocked(&bus, 2.0, &delayed, [-9.0, 0.0, -1.0]);
    e.bus_play_3d(&idle, &plain, [1.0, 0.0, 0.0], STILL);
    out.extend(e.mix(1200));
    e.source(v, [-3.0, 0.0, 3.0], STILL);
    e.update();
    out.extend(e.mix(1300));
    out
}

/// A moved and a zero speaker (channel 0, which every output has), and a
/// turned, tilted listener.
fn speakers<E: Engine>(e: &mut E) -> Vec<f32> {
    let mut source = tone(30000, 1, 44100.0, 53);
    source.set_3d_attenuation(Attenuation::InverseDistance, 1.0);
    let wide = tone(30000, 2, 44100.0, 19);
    let options = PlayOptions::default();
    let mut out = Vec::new();
    e.listener([0.0, 1.0, 0.0], [1.0, 0.0, 1.0], [0.2, 1.0, 0.0], STILL);
    let a = e.play_3d(&source, [2.0, 0.0, 3.0], STILL, options);
    e.play_3d(&wide, [-1.0, 0.0, -1.0], STILL, options);
    out.extend(e.mix(500));
    e.speaker(0, [1.0, 0.5, -3.0]);
    e.source(a, [-4.0, 2.0, 1.0], STILL);
    e.update();
    out.extend(e.mix(700));
    // A zero speaker hears every direction at full volume.
    e.speaker(0, STILL);
    e.update();
    out.extend(e.mix(300));
    out
}

/// A wall at x = 0 and an attenuator that cuts off past the maximum
/// distance, on the game thread.
#[derive(Debug)]
pub(super) struct Wall;

impl Collider for Wall {
    fn collide(&self, voice: &SpatialVoice, user_data: i32) -> f32 {
        if voice.position[0] > 0.0 {
            1.0 / user_data as f32
        } else {
            1.0
        }
    }
}

#[derive(Debug)]
struct Cutoff;

impl Attenuator for Cutoff {
    fn attenuate(&self, distance: f32, _min: f32, max: f32, rolloff: f32) -> f32 {
        if distance < max { rolloff } else { 0.0 }
    }
}

/// Colliders and attenuators, which the controller calls itself.
fn custom<E: Engine>(e: &mut E) -> Vec<f32> {
    let mut walled = tone(30000, 1, 44100.0, 31);
    walled.set_3d_collider(Some(std::sync::Arc::new(Wall)), 4);
    walled.set_3d_attenuation(Attenuation::InverseDistance, 1.0);
    let mut cut = tone(30000, 2, 44100.0, 23);
    cut.set_3d_attenuator(Some(std::sync::Arc::new(Cutoff)));
    cut.set_3d_min_max_distance(1.0, 10.0);
    cut.set_inaudible_behavior(true, false);
    let options = PlayOptions::default();
    let mut out = Vec::new();
    let a = e.play_3d(&walled, [-2.0, 0.0, -1.0], STILL, options);
    let b = e.play_3d(&cut, [0.0, 0.0, -5.0], STILL, options);
    out.extend(e.mix(600));
    e.source(a, [2.0, 0.0, -1.0], STILL);
    e.source(b, [0.0, 0.0, -50.0], STILL);
    e.update();
    out.extend(e.mix(600));
    e.source(b, [3.0, 0.0, 0.0], STILL);
    e.source_settings(b, 1.0, (1.0, 2.0));
    e.update();
    out.extend(e.mix(400));
    e.source_settings(b, 1.0, (1.0, 20.0));
    e.update();
    out.extend(e.mix(400));
    out
}

/// Filters whose memory grows as Doppler rises after the play: a flanger
/// running, and echo and DC removal voices paused until the factor rose.
fn rising<E: Engine>(e: &mut E) -> Vec<f32> {
    use crate::filters::{DcRemovalFilter, EchoFilter, Filter, FlangerFilter};
    let mut flanger = FlangerFilter::default();
    flanger.set_params(0.1, 1.0).unwrap();
    let filters: [std::sync::Arc<dyn Filter>; 3] = [
        std::sync::Arc::new(flanger),
        std::sync::Arc::new(EchoFilter::default()),
        std::sync::Arc::new(DcRemovalFilter::default()),
    ];
    let paused = PlayOptions {
        paused: true,
        ..PlayOptions::default()
    };
    let mut ids = Vec::new();
    for (i, filter) in filters.into_iter().enumerate() {
        let mut sound = tone(30000, 1, 44100.0, 43 + i);
        sound.set_filter(0, Some(filter));
        let options = if i == 0 {
            PlayOptions::default()
        } else {
            paused
        };
        ids.push(e.play_3d(&sound, [i as f32, 0.0, -20.0], STILL, options));
    }
    let mut out = e.mix(700);
    for (step, speed) in [200.0, 260.0, 300.0].into_iter().enumerate() {
        for (i, &id) in ids.iter().enumerate() {
            e.source(id, [i as f32, 0.0, -20.0], [0.0, 0.0, speed]);
        }
        e.update();
        if step == 1 {
            e.pause(ids[1], false);
            e.pause(ids[2], false);
        }
        out.extend(e.mix(600));
    }
    out
}

fn config(channels: u32, sample_rate: u32, left_handed_3d: bool) -> MixerConfig {
    MixerConfig {
        channels,
        sample_rate,
        left_handed_3d,
        ..MixerConfig::default()
    }
}

#[test]
fn spatial_renders_the_same_samples_as_the_mixer() {
    for config in [
        config(2, 44100, false),
        config(6, 48000, false),
        config(2, 22050, true),
        config(8, 44100, true),
        config(1, 44100, false),
        config(4, 44100, false),
    ] {
        let (mut mixer, mut realtime) = both(config);
        compare(config, moving(&mut mixer), moving(&mut realtime));
        let (mut mixer, mut realtime) = both(config);
        let offline = delays_and_buses(&mut mixer);
        compare(config, offline, delays_and_buses(&mut realtime));
        let (mut mixer, mut realtime) = both(config);
        compare(config, speakers(&mut mixer), speakers(&mut realtime));
        let (mut mixer, mut realtime) = both(config);
        compare(config, custom(&mut mixer), custom(&mut realtime));
        let (mut mixer, mut realtime) = both(config);
        compare(config, rising(&mut mixer), rising(&mut realtime));
    }
}
