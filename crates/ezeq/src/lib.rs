//! Ezeq audio plugin entry points and host integration.

// The current upstream VST3 bindings emit this warning on Rust 1.99.
#![allow(semicolon_in_expressions_from_non_local_macros)]

mod dsp;
mod editor;
mod params;

use dsp::{spectrum_channel, DynamicsOutput, EqEngine, SpectrumOutput};
use nih_plug::prelude::*;
use params::EzeqParams;
use std::sync::Arc;

pub struct Ezeq {
    params: Arc<EzeqParams>,
    engine: EqEngine,
    spectrum: Arc<SpectrumOutput>,
    dynamics: Arc<DynamicsOutput>,
}

impl Default for Ezeq {
    fn default() -> Self {
        let (spectrum_input, spectrum_output) = spectrum_channel();
        let spectrum = Arc::new(spectrum_output);
        let dynamics = Arc::new(DynamicsOutput::default());

        Self {
            params: Arc::new(EzeqParams::default()),
            engine: EqEngine::new(spectrum_input, dynamics.clone()),
            spectrum,
            dynamics,
        }
    }
}

impl Plugin for Ezeq {
    const NAME: &'static str = "EzEq";
    const VENDOR: &'static str = "EzEq Audio";
    const URL: &'static str = env!("CARGO_PKG_REPOSITORY");
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            aux_input_ports: &[new_nonzero_u32(1)],
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
        editor::create(
            self.params.clone(),
            self.spectrum.clone(),
            self.dynamics.clone(),
        )
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
            self.params.as_ref(),
        );
        true
    }

    fn reset(&mut self) {
        self.engine.reset(self.params.as_ref());
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.engine
            .process(buffer, aux.inputs.first(), self.params.as_ref());
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Ezeq {
    const CLAP_ID: &'static str = "com.pauloostenrijk.ezeq";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A modern parametric equalizer");
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

#[cfg(test)]
mod tests {
    use super::*;

    const _: () = assert!(Ezeq::SAMPLE_ACCURATE_AUTOMATION);

    #[test]
    fn advertises_mono_and_stereo_layouts() {
        assert_eq!(Ezeq::AUDIO_IO_LAYOUTS.len(), 2);

        let channel_counts: Vec<(u32, u32)> = Ezeq::AUDIO_IO_LAYOUTS
            .iter()
            .map(|layout| {
                (
                    layout.main_input_channels.unwrap().get(),
                    layout.main_output_channels.unwrap().get(),
                )
            })
            .collect();
        assert_eq!(channel_counts, [(2, 2), (1, 1)]);

        let sidechain_channels: Vec<u32> = Ezeq::AUDIO_IO_LAYOUTS
            .iter()
            .map(|layout| layout.aux_input_ports[0].get())
            .collect();
        assert_eq!(sidechain_channels, [2, 1]);
    }
}
