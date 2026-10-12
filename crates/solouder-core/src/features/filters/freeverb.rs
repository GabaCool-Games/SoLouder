// Ported from SoLoud src/filter/soloud_freeverbfilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder. The reverb
// model is based on code written by Jezar at Dreampoint, June 2000, which was
// placed in the public domain.

use super::{Filter, FilterInstance, FilterParams, InvalidFilterParams, ParamType};

/// Jezar's Freeverb stereo reverb (`FreeverbFilter`). Only the first two
/// channels are processed; mono audio is left unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreeverbFilter {
    mode: f32,
    room_size: f32,
    damp: f32,
    width: f32,
}

impl FreeverbFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;
    /// Freeze mode: at 0.5 or above the reverb tail holds and takes no
    /// input.
    pub const FREEZE: u32 = 1;
    /// Room size, which sets the comb feedback.
    pub const ROOMSIZE: u32 = 2;
    /// High-frequency damping.
    pub const DAMP: u32 = 3;
    /// Stereo width.
    pub const WIDTH: u32 = 4;

    /// Changes the settings that new instances start with
    /// (`FreeverbFilter::setParams`). Freeze must be 0 to 1, room size and
    /// width above 0, and damping at least 0.
    pub fn set_params(
        &mut self,
        freeze: f32,
        room_size: f32,
        damp: f32,
        width: f32,
    ) -> Result<(), InvalidFilterParams> {
        if !((0.0..=1.0).contains(&freeze) && room_size > 0.0 && damp >= 0.0 && width > 0.0) {
            return Err(InvalidFilterParams);
        }
        *self = Self {
            mode: freeze,
            room_size,
            damp,
            width,
        };
        Ok(())
    }
}

impl Default for FreeverbFilter {
    /// Not frozen, room size 0.5, damping 0.5, width 1.
    fn default() -> Self {
        Self {
            mode: 0.0,
            room_size: 0.5,
            damp: 0.5,
            width: 1.0,
        }
    }
}

impl Filter for FreeverbFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        let mut params = FilterParams::new(5);
        let values = params.values_mut();
        values[Self::FREEZE as usize] = self.mode;
        values[Self::ROOMSIZE as usize] = self.room_size;
        values[Self::DAMP as usize] = self.damp;
        values[Self::WIDTH as usize] = self.width;
        Box::new(FreeverbInstance {
            params,
            model: Box::new(Model::new()),
            // The model starts with its own defaults; the settings apply at
            // the first filter call (reference patch 0040).
            pending: true,
        })
    }

    fn param_count(&self) -> u32 {
        5
    }

    fn param_name(&self, index: u32) -> Option<&'static str> {
        Some(match index {
            Self::FREEZE => "Freeze",
            Self::ROOMSIZE => "Room size",
            Self::DAMP => "Damp",
            Self::WIDTH => "Width",
            _ => "Wet",
        })
    }

    fn param_type(&self, index: u32) -> ParamType {
        if index == Self::FREEZE {
            ParamType::Bool
        } else {
            ParamType::Float
        }
    }
}

#[derive(Debug)]
struct FreeverbInstance {
    params: FilterParams,
    model: Box<Model>,
    pending: bool,
}

impl FilterInstance for FreeverbInstance {
    fn params(&self) -> &FilterParams {
        &self.params
    }

    fn params_mut(&mut self) -> &mut FilterParams {
        &mut self.params
    }

    fn filter(
        &mut self,
        buffer: &mut [f32],
        frames: usize,
        stride: usize,
        channels: usize,
        _sample_rate: f32,
        time: f64,
    ) {
        self.params.update(time);
        if self.pending || self.params.changed() != 0 {
            let p = self.params.values();
            let model = &mut self.model;
            model.damp = p[FreeverbFilter::DAMP as usize] * SCALE_DAMP;
            model.mode = p[FreeverbFilter::FREEZE as usize];
            model.room_size = p[FreeverbFilter::ROOMSIZE as usize] * SCALE_ROOM + OFFSET_ROOM;
            model.width = p[FreeverbFilter::WIDTH as usize];
            model.wet = p[FreeverbFilter::WET as usize] * SCALE_WET;
            model.dry = (1.0 - p[FreeverbFilter::WET as usize]) * SCALE_DRY;
            model.dirty = true;
            self.params.clear_changed();
            self.pending = false;
        }
        if channels < 2 {
            return;
        }
        let (left, right) = buffer.split_at_mut(stride);
        self.model
            .process(&mut left[..frames], &mut right[..frames]);
    }
}

const FIXED_GAIN: f32 = 0.015;
const SCALE_WET: f32 = 3.0;
const SCALE_DRY: f32 = 2.0;
const SCALE_DAMP: f32 = 0.4;
const SCALE_ROOM: f32 = 0.28;
const OFFSET_ROOM: f32 = 0.7;
const FREEZE_MODE: f32 = 0.5;
const STEREO_SPREAD: usize = 23;
const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];

#[derive(Debug)]
struct Comb {
    buffer: Vec<f32>,
    index: usize,
    feedback: f32,
    store: f32,
    damp1: f32,
    damp2: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buffer: vec![0.0; len],
            index: 0,
            feedback: 0.0,
            store: 0.0,
            damp1: 0.0,
            damp2: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.index];
        self.store = (output * self.damp2) + (self.store * self.damp1);
        self.buffer[self.index] = input + (self.store * self.feedback);
        self.index = (self.index + 1) % self.buffer.len();
        output
    }
}

#[derive(Debug)]
struct Allpass {
    buffer: Vec<f32>,
    index: usize,
}

impl Allpass {
    const FEEDBACK: f32 = 0.5;

    fn process(&mut self, input: f32) -> f32 {
        let out = self.buffer[self.index];
        self.buffer[self.index] = input + (out * Self::FEEDBACK);
        self.index = (self.index + 1) % self.buffer.len();
        -input + out
    }
}

/// `FreeverbImpl::Revmodel`. Its constructor's own settings are always
/// replaced before the first `process`, so only the state that outlives
/// them is kept.
#[derive(Debug)]
struct Model {
    room_size: f32,
    damp: f32,
    wet: f32,
    dry: f32,
    width: f32,
    mode: f32,
    gain: f32,
    wet1: f32,
    wet2: f32,
    dirty: bool,
    combs: [[Comb; 8]; 2],
    allpasses: [[Allpass; 4]; 2],
}

impl Model {
    fn new() -> Self {
        let comb =
            |side: usize| std::array::from_fn(|i| Comb::new(COMB_TUNING[i] + side * STEREO_SPREAD));
        let allpass = |side: usize| {
            std::array::from_fn(|i| Allpass {
                buffer: vec![0.0; ALLPASS_TUNING[i] + side * STEREO_SPREAD],
                index: 0,
            })
        };
        Self {
            room_size: 0.0,
            damp: 0.0,
            wet: 0.0,
            dry: 0.0,
            width: 0.0,
            mode: 0.0,
            gain: 0.0,
            wet1: 0.0,
            wet2: 0.0,
            dirty: true,
            combs: [comb(0), comb(1)],
            allpasses: [allpass(0), allpass(1)],
        }
    }

    fn update(&mut self) {
        self.wet1 = self.wet * (self.width / 2.0 + 0.5);
        self.wet2 = self.wet * ((1.0 - self.width) / 2.0);
        let (feedback, damp) = if self.mode >= FREEZE_MODE {
            self.gain = 0.0;
            (1.0, 0.0)
        } else {
            self.gain = FIXED_GAIN;
            (self.room_size, self.damp)
        };
        for comb in self.combs.iter_mut().flatten() {
            comb.feedback = feedback;
            comb.damp1 = damp;
            comb.damp2 = 1.0 - damp;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.dirty {
            self.update();
        }
        self.dirty = false;
        let [combs_l, combs_r] = &mut self.combs;
        let [passes_l, passes_r] = &mut self.allpasses;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = (*l + *r) * self.gain;
            let (mut out_l, mut out_r) = (0.0_f32, 0.0_f32);
            for (comb_l, comb_r) in combs_l.iter_mut().zip(combs_r.iter_mut()) {
                out_l += comb_l.process(input);
                out_r += comb_r.process(input);
            }
            for (pass_l, pass_r) in passes_l.iter_mut().zip(passes_r.iter_mut()) {
                out_l = pass_l.process(out_l);
                out_r = pass_r.process(out_r);
            }
            *l = out_l * self.wet1 + out_r * self.wet2 + *l * self.dry;
            *r = out_r * self.wet1 + out_l * self.wet2 + *r * self.dry;
        }
    }
}
