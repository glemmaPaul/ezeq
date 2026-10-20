mod biquad;
mod spectrum;

pub(crate) use spectrum::{SpectrumOutput, MIN_DB, SPECTRUM_BINS};

use self::spectrum::SpectrumAnalyzer;
use nih_plug::prelude::Buffer;
use std::sync::Arc;

/// Real-time EQ processing state.
///
/// Audio remains a transparent pass-through. The analyzer only observes the
/// samples and publishes display data through atomics.
pub(crate) struct EqEngine {
    sample_rate: f32,
    max_buffer_size: usize,
    analyzer: SpectrumAnalyzer,
}

impl EqEngine {
    pub(crate) fn new(spectrum: Arc<SpectrumOutput>) -> Self {
        Self {
            sample_rate: 0.0,
            max_buffer_size: 0,
            analyzer: SpectrumAnalyzer::new(spectrum),
        }
    }

    pub(crate) fn prepare(&mut self, sample_rate: f32, max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.max_buffer_size = max_buffer_size;
        self.analyzer.prepare(sample_rate);
    }

    pub(crate) fn reset(&mut self) {
        self.analyzer.reset();
    }

    #[inline]
    pub(crate) fn process(&mut self, buffer: &mut Buffer) {
        debug_assert!(self.sample_rate > 0.0);
        debug_assert!(self.max_buffer_size > 0);

        let channels = buffer.as_slice_immutable();
        if channels.is_empty() {
            return;
        }

        let channel_gain = 1.0 / channels.len() as f32;
        for sample_index in 0..buffer.samples() {
            let mut mono_sample = 0.0;
            for channel in channels {
                mono_sample += channel[sample_index];
            }

            self.analyzer.push_sample(mono_sample * channel_gain);
        }
    }
}
