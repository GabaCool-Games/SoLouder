// Ported from SoLoud src/core/soloud_core_voicegroup.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::control::ControlError;
use super::handle::{GROUP_MARK, Handle};
use super::mixer::Mixer;
/// Voice groups a mixer can hold at once.
const MAX_GROUPS: usize = 4096;

/// Voice groups: each a list of voice handles in the order they were added.
/// Handles of stopped voices stay until the group is trimmed.
#[derive(Debug, Default)]
pub(crate) struct VoiceGroups {
    groups: Vec<Option<Vec<Handle>>>,
}

impl VoiceGroups {
    /// The group `handle` names, if it is a voice group handle and the group
    /// exists (`voiceGroupHandleToArray_internal`).
    pub fn index_of(&self, handle: Handle) -> Option<usize> {
        if !handle.has_group_mark() {
            return None;
        }
        let index = (handle.raw() & 0xfff) as usize;
        self.groups.get(index)?.as_ref().map(|_| index)
    }

    /// The `i`th handle in `group`.
    pub fn member(&self, group: usize, i: usize) -> Option<Handle> {
        self.groups[group].as_ref()?.get(i).copied()
    }
}

/// Voice groups: a group handle can be passed to every call that changes
/// voices, and changes each voice in the group. Calls that read a voice read
/// the group's first voice; [`Mixer::is_valid_voice_handle`] is false for a
/// group.
impl Mixer {
    /// Creates an empty voice group, or returns [`Handle::INVALID`] when all
    /// 4096 exist (`Soloud::createVoiceGroup`). The lowest free index is used.
    pub fn create_voice_group(&mut self) -> Handle {
        let groups = &mut self.voices.groups.groups;
        let index = match groups.iter().position(Option::is_none) {
            Some(index) => index,
            None if groups.len() < MAX_GROUPS => {
                groups.push(None);
                groups.len() - 1
            }
            None => return Handle::INVALID,
        };
        groups[index] = Some(Vec::new());
        Handle::from_raw(GROUP_MARK | index as u32)
    }

    /// Destroys a voice group; its voices keep playing.
    pub fn destroy_voice_group(&mut self, group: Handle) -> Result<(), ControlError> {
        let index = self.group_index(group)?;
        self.voices.groups.groups[index] = None;
        Ok(())
    }

    /// Adds a voice to a group. A stopped voice, or one already in the
    /// group, is not added and is not an error. Stopped voices are removed
    /// from the group first.
    pub fn add_voice_to_group(&mut self, group: Handle, voice: Handle) -> Result<(), ControlError> {
        let index = self.group_index(group)?;
        if !self.is_valid_voice_handle(voice) {
            return Ok(());
        }
        self.trim_voice_group(index);
        if let Some(members) = self.voices.groups.groups[index].as_mut()
            && !members.contains(&voice)
        {
            members.push(voice);
        }
        Ok(())
    }

    /// Whether `handle` names an existing voice group.
    pub fn is_voice_group(&self, handle: Handle) -> bool {
        self.voices.groups.index_of(handle).is_some()
    }

    /// Whether a group has no playing voices; true for a handle that is not
    /// a group. Stopped voices are removed from the group.
    pub fn is_voice_group_empty(&mut self, group: Handle) -> bool {
        let Ok(index) = self.group_index(group) else {
            return true;
        };
        self.trim_voice_group(index);
        self.voices.groups.groups[index]
            .as_ref()
            .is_none_or(Vec::is_empty)
    }

    fn group_index(&self, group: Handle) -> Result<usize, ControlError> {
        self.voices
            .groups
            .index_of(group)
            .ok_or(ControlError::InvalidVoiceGroup)
    }

    /// Removes the handles of stopped voices, keeping the order of the rest
    /// (`trimVoiceGroup_internal`).
    fn trim_voice_group(&mut self, index: usize) {
        let Some(mut members) = self.voices.groups.groups[index].take() else {
            return;
        };
        members.retain(|&member| self.voices.slot_of(member).is_some());
        self.voices.groups.groups[index] = Some(members);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::Pcm;
    use crate::features::playback::{MixerConfig, Sound};

    fn setup() -> (Mixer, Sound) {
        let mixer = Mixer::new(MixerConfig::default()).unwrap();
        let pcm = Pcm::from_planar(vec![0.5; 4096], 1, 44100.0).unwrap();
        (mixer, Sound::new(pcm))
    }

    #[test]
    fn handles_reuse_the_lowest_free_index() {
        let (mut mixer, _) = setup();
        let a = mixer.create_voice_group();
        let b = mixer.create_voice_group();
        assert_eq!((a.raw(), b.raw()), (0xffff_f000, 0xffff_f001));
        mixer.destroy_voice_group(a).unwrap();
        assert!(!mixer.is_voice_group(a));
        assert_eq!(
            mixer.destroy_voice_group(a),
            Err(ControlError::InvalidVoiceGroup)
        );
        assert_eq!(mixer.create_voice_group(), a);
    }

    #[test]
    fn at_most_4096_groups() {
        let (mut mixer, _) = setup();
        for _ in 0..MAX_GROUPS {
            assert_ne!(mixer.create_voice_group(), Handle::INVALID);
        }
        assert_eq!(mixer.create_voice_group(), Handle::INVALID);
        assert_eq!(
            Handle::from_raw(u32::MAX),
            Handle::from_raw(GROUP_MARK | 4095)
        );
        assert!(mixer.is_voice_group(Handle::from_raw(u32::MAX)));
    }

    #[test]
    fn group_calls_reach_every_member() {
        let (mut mixer, sound) = setup();
        let group = mixer.create_voice_group();
        let a = mixer.play(&sound);
        let b = mixer.play(&sound);
        let outside = mixer.play(&sound);
        for h in [a, b, a] {
            mixer.add_voice_to_group(group, h).unwrap();
        }
        mixer.add_voice_to_group(group, group).unwrap();
        mixer.set_volume(group, 0.25);
        assert_eq!(mixer.volume(a), Some(0.25));
        assert_eq!(mixer.volume(b), Some(0.25));
        assert_eq!(mixer.volume(outside), Some(1.0));
        assert_eq!(mixer.volume(group), Some(0.25));
        assert!(!mixer.is_valid_voice_handle(group));

        mixer.stop(a);
        // The first member stopped and the group was not trimmed yet.
        assert_eq!(mixer.volume(group), None);
        assert!(!mixer.is_voice_group_empty(group));
        assert_eq!(mixer.volume(group), Some(0.25));
        mixer.stop(group);
        assert!(!mixer.is_valid_voice_handle(b));
        assert!(mixer.is_valid_voice_handle(outside));
        assert!(mixer.is_voice_group_empty(group));
        assert!(mixer.is_voice_group_empty(Handle::from_raw(0xffff_f123)));
    }
}
