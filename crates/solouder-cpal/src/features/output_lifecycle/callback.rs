//! What runs in the device callback, and how the renderer gets back to its
//! owner when the callback is dropped.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use cpal::{FromSample, Sample};
use rtrb::Producer;
use solouder_core::Renderer;

/// State the callback and error callback share with [`Output`](super::Output).
#[derive(Debug, Default)]
pub(crate) struct OutputShared {
    pub panicked: AtomicBool,
    /// An [`OutputState`](super::OutputState) code.
    pub state: AtomicU8,
    pub errors: AtomicU64,
    pub callbacks: AtomicU64,
    /// Frames the most recent callback asked for.
    pub callback_frames: AtomicU64,
}

/// Holds the renderer inside the callback closure. However the closure is
/// dropped, the renderer is handed back through `back` instead of being freed
/// on the audio thread.
pub(crate) struct ReturnOnDrop {
    renderer: Option<Renderer>,
    back: Producer<Renderer>,
}

impl ReturnOnDrop {
    pub fn new(renderer: Renderer, back: Producer<Renderer>) -> Self {
        Self {
            renderer: Some(renderer),
            back,
        }
    }
}

impl Drop for ReturnOnDrop {
    fn drop(&mut self) {
        if let Some(renderer) = self.renderer.take() {
            // The slot holds one renderer and only this sends; if the owner
            // is gone the renderer is dropped here.
            let _ = self.back.push(renderer);
        }
    }
}

/// The audio callback: renders through a preallocated `f32` scratch buffer
/// and converts to the device's sample type.
pub(crate) struct Callback {
    holder: ReturnOnDrop,
    scratch: Vec<f32>,
    channels: usize,
    shared: Arc<OutputShared>,
}

impl Callback {
    /// `scratch_frames` bounds one render; longer callbacks render in parts.
    pub fn new(holder: ReturnOnDrop, scratch_frames: usize, shared: Arc<OutputShared>) -> Self {
        let channels = holder
            .renderer
            .as_ref()
            .map_or(1, |renderer| renderer.config().channels as usize);
        Self {
            holder,
            scratch: vec![0.0; scratch_frames.max(1) * channels],
            channels,
            shared,
        }
    }

    /// Fills `data` with the next frames. After a panic in the renderer the
    /// output stays silent: the mixer's state can no longer be trusted.
    pub fn fill<T: Sample + FromSample<f32>>(&mut self, data: &mut [T]) {
        self.shared.callbacks.fetch_add(1, Ordering::Relaxed);
        self.shared
            .callback_frames
            .store((data.len() / self.channels) as u64, Ordering::Relaxed);
        let renderer = match self.holder.renderer.as_mut() {
            Some(renderer) if !self.shared.panicked.load(Ordering::Relaxed) => renderer,
            _ => return data.fill(T::EQUILIBRIUM),
        };
        let chunk_len = self.scratch.len() / self.channels * self.channels;
        for chunk in data.chunks_mut(chunk_len) {
            let scratch = &mut self.scratch[..chunk.len()];
            let rendered = catch_unwind(AssertUnwindSafe(|| renderer.render(scratch)));
            if rendered.is_err() {
                self.shared.panicked.store(true, Ordering::Relaxed);
                return data.fill(T::EQUILIBRIUM);
            }
            for (out, &sample) in chunk.iter_mut().zip(scratch.iter()) {
                *out = T::from_sample(sample);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use solouder_core::realtime::{RealtimeConfig, split};
    use solouder_core::{Mixer, MixerConfig, Pcm, Sound};

    use super::*;

    fn callback(
        scratch_frames: usize,
    ) -> (
        Callback,
        solouder_core::Controller,
        rtrb::Consumer<Renderer>,
    ) {
        let mixer = Mixer::new(MixerConfig::default()).unwrap();
        let (mut controller, renderer) = split(mixer, RealtimeConfig::default());
        let samples = vec![0.5; 20_000];
        let sound = Sound::new(Pcm::from_planar(samples, 1, 44100.0).unwrap());
        controller.play(&sound).unwrap();
        let (back, returned) = rtrb::RingBuffer::new(1);
        let holder = ReturnOnDrop::new(renderer, back);
        let callback = Callback::new(holder, scratch_frames, Arc::default());
        (callback, controller, returned)
    }

    #[test]
    fn long_callbacks_render_in_parts_and_convert() {
        let (mut cb, ..) = callback(64);
        let mut converted = vec![0i16; 2 * 1000];
        cb.fill(&mut converted);

        let (mut reference, ..) = callback(64);
        let renderer = reference.holder.renderer.as_mut().unwrap();
        let mut expected = Vec::new();
        for frames in [64; 15].into_iter().chain([40]) {
            let mut part = vec![0.0f32; 2 * frames];
            renderer.render(&mut part);
            expected.extend(part.iter().map(|&s| i16::from_sample(s)));
        }
        assert_eq!(converted, expected);
        assert!(converted.iter().any(|&s| s != 0));
        assert_eq!(cb.shared.callbacks.load(Ordering::Relaxed), 1);
        assert_eq!(cb.shared.callback_frames.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn a_poisoned_output_is_silent() {
        let (mut cb, ..) = callback(256);
        cb.shared.panicked.store(true, Ordering::Relaxed);
        let mut data = vec![7u16; 64];
        cb.fill(&mut data);
        assert!(data.iter().all(|&s| s == u16::EQUILIBRIUM));
    }

    #[test]
    fn dropping_the_callback_returns_the_renderer() {
        let (cb, _controller, mut returned) = callback(256);
        drop(cb);
        assert!(returned.pop().is_ok());
    }
}
