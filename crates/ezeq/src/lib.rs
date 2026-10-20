//! Ezeq audio plugin entry points and host integration.

// The current upstream VST3 bindings emit this warning on Rust 1.99.
#![allow(semicolon_in_expressions_from_non_local_macros)]

mod dsp;
mod editor;
mod params;

use dsp::{EqEngine, SpectrumOutput};
use nih_plug::prelude::*;
use params::EzeqParams;
use std::sync::Arc;

pub struct Ezeq {
    params: Arc<EzeqParams>,
    engine: EqEngine,
    spectrum: Arc<SpectrumOutput>,
}

impl Default for Ezeq {
    fn default() -> Self {
        let spectrum = Arc::new(SpectrumOutput::default());

        Self {
            params: Arc::new(EzeqParams::default()),
            engine: EqEngine::new(spectrum.clone()),
            spectrum,
        }
    }
}

impl Plugin for Ezeq {
    const NAME: &'static str = "Ezeq";
    const VENDOR: &'static str = "Ezeq Audio";
    const URL: &'static str = env!("CARGO_PKG_REPOSITORY");
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.spectrum.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.engine.prepare(
            buffer_config.sample_rate,
            buffer_config.max_buffer_size as usize,
        );
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.engine.process(buffer);
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Ezeq {
    const CLAP_ID: &'static str = "com.pauloostenrijk.ezeq";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("A modern parametric equalizer (pass-through scaffold)");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Equalizer,
        ClapFeature::Stereo,
        ClapFeature::Mono,
    ];
}

impl Vst3Plugin for Ezeq {
    // This ID is part of session compatibility and must never change after release.
    const VST3_CLASS_ID: [u8; 16] = *b"EzeqAudioEq00001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Filter];
}

nih_export_clap!(Ezeq);
nih_export_vst3!(Ezeq);
