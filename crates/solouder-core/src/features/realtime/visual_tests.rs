use super::*;
use crate::features::playback::spectrum;
use crate::{Bus, Mixer, MixerConfig, Pcm, PlayOptions, Sound, WAVE_LEN};

fn sound(frames: usize, period: usize) -> Sound {
    let samples = (0..frames * 2)
        .map(|i| (i % period) as f32 / period as f32 - 0.5)
        .collect();
    let mut sound = Sound::new(Pcm::from_planar(samples, 2, 44100.0).unwrap());
    sound.set_looping(true);
    sound
}

fn mixer() -> Mixer {
    Mixer::new(MixerConfig::default()).unwrap()
}

/// The controller reads what the mixer records: the mix's data and a
/// bus's, enabled before and while it plays, until the bus stops.
#[test]
fn the_controller_reads_what_the_mixer_records() {
    let config = RealtimeConfig {
        visualized_buses: 2,
        ..RealtimeConfig::default()
    };
    let (mut c, mut renderer) = split(mixer(), config);
    let mut offline = mixer();
    let mut early = Bus::new();
    early.set_visualization(true);
    let mut late = Bus::new();
    late.set_channels(4).unwrap();
    let (tone, buzz) = (sound(3000, 41), sound(2000, 7));
    let options = PlayOptions::default();
    let early_id = c.play(&early).unwrap();
    c.play(&late).unwrap();
    c.bus_play(&early, &tone, options).unwrap();
    c.bus_play(&late, &buzz, options).unwrap();
    c.set_visualization(true).unwrap();
    let early_handle = offline.play(&early);
    offline.play(&late);
    offline.bus_play(&early, &tone, options);
    offline.bus_play(&late, &buzz, options);
    offline.set_visualization(true);
    assert_eq!(c.wave(), [0.0; WAVE_LEN]);
    let (mut out, mut expected) = (vec![0.0; 2 * 4096], vec![0.0; 2 * 4096]);
    for (round, frames) in [700, 100, 0, 256, 3].into_iter().enumerate() {
        if round == 1 {
            c.set_bus_visualization(&mut late, true).unwrap();
            offline.set_bus_visualization(&mut late.clone(), true);
        }
        renderer.render(&mut out[..2 * frames]);
        offline.mix_interleaved(&mut expected[..2 * frames]);
        assert_eq!(c.wave(), offline.wave(), "round {round}");
        assert_eq!(c.fft(), offline.fft());
        for channel in 0..10 {
            assert_eq!(
                c.approximate_volume(channel),
                offline.approximate_volume(channel)
            );
            for bus in [&early, &late] {
                let volume = c.bus_approximate_volume(bus, channel);
                assert_eq!(volume, offline.bus_approximate_volume(bus, channel));
            }
        }
        assert_eq!(c.bus_wave(&early), offline.bus_wave(&early));
        // The late bus is published once its visualization is enabled.
        let late_wave = (round > 0).then(|| offline.bus_wave(&late)).flatten();
        assert_eq!(c.bus_fft(&late), late_wave.map(|w| spectrum(&w)));
    }
    c.stop(early_id).unwrap();
    offline.stop(early_handle);
    renderer.render(&mut out[..64]);
    assert_eq!((c.bus_wave(&early), offline.bus_wave(&early)), (None, None));
    assert_eq!(c.bus_approximate_volume(&early, 0), 0.0);
    // Off keeps the last data.
    let wave = c.wave();
    c.set_visualization(false).unwrap();
    renderer.render(&mut out[..512]);
    assert_eq!(c.wave(), wave);
}

/// Each bus that records takes one of the configured slots, whether it is
/// enabled through the controller or played already recording.
#[test]
fn buses_beyond_the_slots_are_refused() {
    let config = RealtimeConfig {
        visualized_buses: 1,
        ..RealtimeConfig::default()
    };
    let (mut c, _renderer) = split(mixer(), config);
    let mut first = Bus::new();
    c.set_bus_visualization(&mut first, true).unwrap();
    c.set_bus_visualization(&mut first, false).unwrap();
    c.set_bus_visualization(&mut first, true).unwrap();
    c.play(&first).unwrap();
    let mut second = Bus::new();
    // Turning it off needs no slot.
    c.set_bus_visualization(&mut second, false).unwrap();
    let refused = Err(ControllerError::TooManyVisualizedBuses);
    assert_eq!(c.set_bus_visualization(&mut second, true), refused);
    assert!(!second.visualization());
    second.set_visualization(true);
    assert_eq!(c.play(&second).map(|_| ()), refused);
    assert_eq!(c.voice_ids_in_use(), 1);
    // Without the flag it plays, and has no data.
    second.set_visualization(false);
    c.play(&second).unwrap();
    assert_eq!(c.bus_wave(&second), None);
}

/// Turning a bus's visualization off releases its slot: its last data stays
/// readable until another bus takes the slot over, and the new bus never
/// reads the old one's data.
#[test]
fn a_released_slot_goes_to_the_next_bus() {
    let config = RealtimeConfig {
        visualized_buses: 1,
        ..RealtimeConfig::default()
    };
    let (mut c, mut renderer) = split(mixer(), config);
    let mut out = vec![0.0; 2 * 512];
    let (mut first, mut second) = (Bus::new(), Bus::new());
    let options = PlayOptions::default();
    for (bus, sound) in [(&first, sound(3000, 41)), (&second, sound(2000, 7))] {
        c.play(bus).unwrap();
        c.bus_play(bus, &sound, options).unwrap();
    }
    c.set_bus_visualization(&mut first, true).unwrap();
    renderer.render(&mut out);
    let first_wave = c.bus_wave(&first).expect("first is watched");
    c.set_bus_visualization(&mut first, false).unwrap();
    renderer.render(&mut out);
    assert_eq!(c.bus_wave(&first), Some(first_wave));
    c.set_bus_visualization(&mut second, true).unwrap();
    // Before the renderer has the change, the slot still holds first's data.
    assert_eq!((c.bus_wave(&first), c.bus_wave(&second)), (None, None));
    renderer.render(&mut out);
    let second_wave = c.bus_wave(&second).expect("second took the slot");
    assert_ne!(second_wave, first_wave);
    assert_eq!(c.bus_wave(&first), None);
    let refused = Err(ControllerError::TooManyVisualizedBuses);
    assert_eq!(c.set_bus_visualization(&mut first, true), refused);
}
