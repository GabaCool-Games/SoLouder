use super::*;
use crate::contracts::Pcm;
use crate::features::playback::sound::Sound;

fn sound() -> Sound {
    Sound::new(Pcm::from_planar(vec![0.0; 16], 1, 44100.0).unwrap())
}

/// Places a voice like `play` does and returns its handle.
fn add(voices: &mut Voices, volume: f32) -> Option<Handle> {
    let slot = voices.find_free()?;
    let mut voice = Voice::new(&sound(), voices.play_index);
    voice.set_volume(volume);
    let handle = Handle::new(slot, voices.play_index);
    voices.play_index += 1;
    voices.slots[slot] = Some(voice);
    voices.dirty = true;
    Some(handle)
}

#[test]
fn handles_go_stale_when_the_slot_is_reused() {
    let mut voices = Voices::new();
    let first = add(&mut voices, 1.0).unwrap();
    voices.stop(voices.slot_of(first).unwrap());
    let second = add(&mut voices, 1.0).unwrap();
    assert_eq!(first.slot(), second.slot());
    assert_eq!(voices.slot_of(first), None);
    assert_eq!(voices.slot_of(second), Some(0));
}

#[test]
fn full_table_steals_the_oldest_unprotected_voice() {
    let mut voices = Voices::new();
    let handles: Vec<_> = (0..VOICE_COUNT)
        .map(|_| add(&mut voices, 1.0).unwrap())
        .collect();
    voices
        .get_mut(handles[0])
        .unwrap()
        .set_flag(flags::PROTECTED, true);
    let stolen = add(&mut voices, 1.0).unwrap();
    assert_eq!(stolen.slot(), Some(1));
    assert!(voices.get(handles[0]).is_some());
    assert!(voices.get(handles[1]).is_none());
}

#[test]
fn full_table_of_protected_voices_has_no_free_slot() {
    let mut voices = Voices::new();
    for _ in 0..VOICE_COUNT {
        let handle = add(&mut voices, 1.0).unwrap();
        voices
            .get_mut(handle)
            .unwrap()
            .set_flag(flags::PROTECTED, true);
    }
    assert_eq!(voices.find_free(), None);
    assert_eq!(voices.count(), VOICE_COUNT);
}

#[test]
fn loudest_and_protected_voices_are_active() {
    let mut voices = Voices::new();
    assert!(voices.set_max_active(2));
    let quiet = add(&mut voices, 0.1).unwrap();
    voices
        .get_mut(quiet)
        .unwrap()
        .set_flag(flags::PROTECTED, true);
    let _soft = add(&mut voices, 0.2).unwrap();
    let loud = add(&mut voices, 0.9).unwrap();
    voices.calc_active();
    assert_eq!(voices.active_count, 2);
    let mut active: Vec<_> = voices.active[..2].iter().map(|&s| s as usize).collect();
    active.sort();
    assert_eq!(active, [quiet.slot().unwrap(), loud.slot().unwrap()]);
    let blocks: Vec<_> = active
        .iter()
        .map(|&s| voices.slots[s].as_ref().unwrap().resample.unwrap())
        .collect();
    assert_ne!(blocks[0].current, blocks[1].current);
}

#[test]
fn paused_voices_are_not_active() {
    let mut voices = Voices::new();
    let handle = add(&mut voices, 1.0).unwrap();
    voices
        .get_mut(handle)
        .unwrap()
        .set_flag(flags::PAUSED, true);
    voices.calc_active();
    assert_eq!(voices.active_count, 0);
}

#[test]
fn max_active_bounds_and_reset() {
    let mut voices = Voices::new();
    assert!(!voices.set_max_active(0));
    assert!(!voices.set_max_active(VOICE_COUNT));
    let handle = add(&mut voices, 1.0).unwrap();
    voices.calc_active();
    assert!(voices.get(handle).unwrap().resample.is_some());
    assert!(voices.set_max_active(VOICE_COUNT - 1));
    assert!(voices.get(handle).unwrap().resample.is_none());
    assert!(voices.dirty);
}

#[test]
fn partial_sort_puts_the_loudest_first() {
    let volumes = [0.3, 0.9, 0.1, 0.5, 0.7, 0.2];
    let mut data: Vec<u32> = (0..volumes.len() as u32).collect();
    partial_sort(&mut data, 3, |i| volumes[i as usize]);
    let mut top: Vec<_> = data[..3].to_vec();
    top.sort();
    assert_eq!(top, [1, 3, 4]);
}
