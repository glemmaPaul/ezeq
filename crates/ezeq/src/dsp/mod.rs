use nih_plug::prelude::Buffer;

/// Real-time EQ processing state.
///
/// The engine is deliberately a transparent pass-through for now. Keeping it
/// separate from the plugin wrapper prevents host integration and UI concerns
/// from leaking into the DSP implementation.
#[derive(Debug, Default)]
pub(crate) struct EqEngine {
    sample_rate: f32,
    max_buffer_size: usize,
}

impl EqEngine {
    pub(crate) fn prepare(&mut self, sample_rate: f32, max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.max_buffer_size = max_buffer_size;
        self.reset();
    }

    pub(crate) fn reset(&mut self) {
        // Filter delay elements will be cleared here.
    }

    #[inline]
    pub(crate) fn process(&mut self, _buffer: &mut Buffer) {
        // Intentionally leave the host-owned in-place buffer untouched.
        //
        // Future DSP code in this function must not allocate, lock, perform
        // file/network I/O, or log. Allocate scratch storage in `prepare()`.
        debug_assert!(self.sample_rate > 0.0);
        debug_assert!(self.max_buffer_size > 0);
    }
}
