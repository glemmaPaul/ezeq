mod biquad;
mod dynamics;
mod spectrum;

pub(crate) use dynamics::DynamicsOutput;
pub(crate) use spectrum::{spectrum_channel, SpectrumOutput, MIN_DB, SPECTRUM_BINS};

use self::biquad::{Biquad, BiquadCoefficients, CoefficientGenerator};
use self::dynamics::{DynamicBand, TriggerFrame};
use self::spectrum::SpectrumInput;
use crate::params::{EqBandParams, EqBandType, EzeqParams, NUM_BANDS};
use nih_plug::prelude::{Buffer, Smoother, SmoothingStyle};
use std::sync::Arc;

const MAX_CHANNELS: usize = 2;
const BYPASS_FADE_MS: f32 = 10.0;

/// Real-time EQ processing state.
///
/// The analyzer observes the input signal while each channel passes through
/// the same eight-band filter configuration with independent delay state.
pub(crate) struct EqEngine {
    sample_rate: f32,
    max_buffer_size: usize,
    bands: [EqBand; NUM_BANDS],
    global_bypass_fade: LinearFade,
    spectrum_input: SpectrumInput,
    dynamics_output: Arc<DynamicsOutput>,
}

impl EqEngine {
    pub(crate) fn new(spectrum_input: SpectrumInput, dynamics_output: Arc<DynamicsOutput>) -> Self {
        Self {
            sample_rate: 0.0,
            max_buffer_size: 0,
            bands: std::array::from_fn(|_| EqBand::default()),
            global_bypass_fade: LinearFade::default(),
            spectrum_input,
            dynamics_output,
        }
    }

    pub(crate) fn prepare(
        &mut self,
        sample_rate: f32,
        max_buffer_size: usize,
        params: &EzeqParams,
    ) {
        self.sample_rate = sample_rate;
        self.max_buffer_size = max_buffer_size;
        for (band, band_params) in self.bands.iter_mut().zip(params.bands()) {
            band.reset(band_params.enabled.value());
            band.invalidate_coefficients();
        }
        self.global_bypass_fade
            .reset(bool_to_mix(params.bypass.value()));
        self.spectrum_input.prepare(sample_rate);
    }

    pub(crate) fn reset(&mut self, params: &EzeqParams) {
        for (band, band_params) in self.bands.iter_mut().zip(params.bands()) {
            band.reset(band_params.enabled.value());
        }
        self.global_bypass_fade
            .reset(bool_to_mix(params.bypass.value()));
        self.spectrum_input.reset();
        self.dynamics_output.reset();
    }

    #[inline]
    pub(crate) fn process(
        &mut self,
        buffer: &mut Buffer,
        sidechain: Option<&Buffer>,
        params: &EzeqParams,
    ) {
        let sidechain_channels = sidechain.map(Buffer::as_slice_immutable);
        self.process_channels(buffer.as_slice(), sidechain_channels, params);
    }

    #[inline]
    fn process_channels(
        &mut self,
        channels: &mut [&mut [f32]],
        sidechain_channels: Option<&[&mut [f32]]>,
        params: &EzeqParams,
    ) {
        debug_assert!(self.sample_rate > 0.0);
        debug_assert!(self.max_buffer_size > 0);

        let channel_count = channels.len();
        if channel_count == 0 {
            self.dynamics_output.reset();
            return;
        }
        debug_assert!(channel_count <= MAX_CHANNELS);
        let sample_count = channels[0].len();
        debug_assert!(channels.iter().all(|channel| channel.len() == sample_count));

        let channel_gain = 1.0 / channel_count as f32;
        let generator = CoefficientGenerator::new(self.sample_rate);

        for sample_index in 0..sample_count {
            let internal_trigger = TriggerFrame::from_channels(channels, sample_index);
            let external_trigger = sidechain_channels
                .map(|sidechain| TriggerFrame::from_channels(sidechain, sample_index))
                .unwrap_or_default();
            let global_dry_mix = self
                .global_bypass_fade
                .next(self.sample_rate, bool_to_mix(params.bypass.value()));
            let mono_sample = internal_trigger.samples[..internal_trigger.channels]
                .iter()
                .copied()
                .sum::<f32>()
                * channel_gain;
            self.spectrum_input.push_sample(mono_sample);

            for (band, band_params) in self.bands.iter_mut().zip(params.bands()) {
                band.update(
                    generator,
                    band_params,
                    self.sample_rate,
                    internal_trigger,
                    external_trigger,
                );
            }

            for (channel_index, channel) in channels.iter_mut().enumerate() {
                if channel_index >= MAX_CHANNELS {
                    break;
                }

                let dry = channel[sample_index];
                let mut wet = dry;
                for band in &mut self.bands {
                    wet = band.process_sample(channel_index, wet);
                }
                channel[sample_index] = crossfade(wet, dry, global_dry_mix);
            }
        }

        for (band_index, band) in self.bands.iter().enumerate() {
            self.dynamics_output
                .set_gain_db(band_index, band.dynamic_gain_db);
        }
    }
}

struct EqBand {
    filters: [Biquad; MAX_CHANNELS],
    enabled: bool,
    wet_mix: f32,
    bypass_fade: LinearFade,
    band_type: Option<EqBandType>,
    frequency_hz: f32,
    gain_db: f32,
    dynamic_gain_db: f32,
    effective_gain_db: f32,
    q: f32,
    dynamics: DynamicBand,
}

impl Default for EqBand {
    fn default() -> Self {
        Self {
            filters: std::array::from_fn(|_| Biquad::default()),
            enabled: false,
            wet_mix: 0.0,
            bypass_fade: LinearFade::default(),
            band_type: None,
            frequency_hz: f32::NAN,
            gain_db: f32::NAN,
            dynamic_gain_db: 0.0,
            effective_gain_db: f32::NAN,
            q: f32::NAN,
            dynamics: DynamicBand::default(),
        }
    }
}

impl EqBand {
    #[inline]
    fn update(
        &mut self,
        generator: CoefficientGenerator,
        params: &EqBandParams,
        sample_rate: f32,
        internal_trigger: TriggerFrame,
        external_trigger: TriggerFrame,
    ) {
        // Advance every smoother even while bypassed so enabling a band never
        // resumes an old ramp.
        let frequency_hz = params.frequency.smoothed.next();
        let gain_db = params.gain.smoothed.next();
        let q = params.q.smoothed.next();
        let enabled = params.enabled.value();
        let band_type = params.band_type.value();

        let topology_changed = self.band_type != Some(band_type);
        let enable_changed = self.enabled != enabled;
        if topology_changed || (enable_changed && enabled) {
            self.reset_filter_state();
        }
        self.wet_mix = self.bypass_fade.next(sample_rate, bool_to_mix(enabled));
        let should_process = enabled || self.bypass_fade.is_smoothing() || self.wet_mix > 0.0;
        let dynamic_gain_db = self.dynamics.next(
            generator,
            params,
            band_type,
            frequency_hz,
            q,
            sample_rate,
            internal_trigger,
            external_trigger,
        );
        let effective_gain_db = gain_db + dynamic_gain_db;

        if should_process
            && (topology_changed
                || enable_changed
                || frequency_hz != self.frequency_hz
                || effective_gain_db != self.effective_gain_db
                || q != self.q)
        {
            let coefficients =
                coefficients_for_band(generator, band_type, frequency_hz, effective_gain_db, q);
            for filter in &mut self.filters {
                filter.set_coefficients(coefficients);
            }
        }

        self.enabled = enabled;
        self.band_type = Some(band_type);
        self.frequency_hz = frequency_hz;
        self.gain_db = gain_db;
        self.dynamic_gain_db = dynamic_gain_db;
        self.effective_gain_db = effective_gain_db;
        self.q = q;
    }

    #[inline]
    fn process_sample(&mut self, channel_index: usize, sample: f32) -> f32 {
        if self.wet_mix > 0.0 {
            let wet = self.filters[channel_index].process_sample(sample);
            crossfade(sample, wet, self.wet_mix)
        } else {
            sample
        }
    }

    fn reset_filter_state(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
    }

    fn reset(&mut self, enabled: bool) {
        self.reset_filter_state();
        self.dynamics.reset();
        self.enabled = enabled;
        self.wet_mix = bool_to_mix(enabled);
        self.dynamic_gain_db = 0.0;
        self.bypass_fade.reset(self.wet_mix);
    }

    fn invalidate_coefficients(&mut self) {
        self.reset_filter_state();
        self.frequency_hz = f32::NAN;
        self.gain_db = f32::NAN;
        self.effective_gain_db = f32::NAN;
        self.q = f32::NAN;
    }
}

struct LinearFade {
    smoother: Smoother<f32>,
    target: f32,
}

impl Default for LinearFade {
    fn default() -> Self {
        Self {
            smoother: Smoother::new(SmoothingStyle::Linear(BYPASS_FADE_MS)),
            target: 0.0,
        }
    }
}

impl LinearFade {
    #[inline]
    fn next(&mut self, sample_rate: f32, target: f32) -> f32 {
        if target != self.target {
            self.target = target;
            self.smoother.set_target(sample_rate, target);
        }
        self.smoother.next()
    }

    fn reset(&mut self, value: f32) {
        self.target = value;
        self.smoother.reset(value);
    }

    #[inline]
    fn is_smoothing(&self) -> bool {
        self.smoother.is_smoothing()
    }
}

#[inline]
fn crossfade(dry: f32, wet: f32, wet_mix: f32) -> f32 {
    (wet - dry).mul_add(wet_mix, dry)
}

#[inline]
const fn bool_to_mix(value: bool) -> f32 {
    if value {
        1.0
    } else {
        0.0
    }
}

fn coefficients_for_band(
    generator: CoefficientGenerator,
    band_type: EqBandType,
    frequency_hz: f32,
    gain_db: f32,
    q: f32,
) -> BiquadCoefficients {
    match band_type {
        EqBandType::HighPass => generator.high_pass(frequency_hz, q),
        EqBandType::LowShelf => generator.low_shelf(frequency_hz, q, gain_db),
        EqBandType::Bell => generator.peaking(frequency_hz, q, gain_db),
        EqBandType::HighShelf => generator.high_shelf(frequency_hz, q, gain_db),
        EqBandType::LowPass => generator.low_pass(frequency_hz, q),
    }
}

pub(crate) fn response_magnitude(params: &EzeqParams, sample_rate: f32, frequency_hz: f32) -> f32 {
    response_magnitude_with_dynamic(params, sample_rate, frequency_hz, &[0.0; NUM_BANDS])
}

pub(crate) fn response_magnitude_with_dynamic(
    params: &EzeqParams,
    sample_rate: f32,
    frequency_hz: f32,
    dynamic_gain_db: &[f32; NUM_BANDS],
) -> f32 {
    if params.bypass.value() {
        return 1.0;
    }

    let generator = CoefficientGenerator::new(sample_rate);
    params
        .bands()
        .into_iter()
        .enumerate()
        .filter(|(_, band)| band.enabled.value())
        .map(|(index, band)| {
            let gain_db = if band.band_type.value().uses_gain() {
                band.gain.value() + dynamic_gain_db[index]
            } else {
                band.gain.value()
            };
            coefficients_for_band(
                generator,
                band.band_type.value(),
                band.frequency.value(),
                gain_db,
                band.q.value(),
            )
            .magnitude_at(sample_rate, frequency_hz)
        })
        .product()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::DynamicSource;

    const SAMPLE_RATE: f32 = 48_000.0;

    fn prepared_engine(max_buffer_size: usize, params: &EzeqParams) -> EqEngine {
        let (spectrum_input, _spectrum_output) = spectrum_channel();
        let mut engine = EqEngine::new(spectrum_input, Arc::new(DynamicsOutput::default()));
        engine.prepare(SAMPLE_RATE, max_buffer_size, params);

        // Start the first band fully wet while keeping its enabled state aligned
        // with the default parameters. The following process call then exercises
        // both the filter state and the click-free fade to the disabled state.
        let band_params = &params.band_1;
        band_params
            .frequency
            .smoothed
            .reset(band_params.frequency.value());
        band_params.gain.smoothed.reset(band_params.gain.value());
        band_params.q.smoothed.reset(band_params.q.value());
        let coefficients = coefficients_for_band(
            CoefficientGenerator::new(SAMPLE_RATE),
            band_params.band_type.value(),
            band_params.frequency.value(),
            12.0,
            band_params.q.value(),
        );
        let band = &mut engine.bands[0];
        for filter in &mut band.filters {
            filter.set_coefficients(coefficients);
        }
        band.enabled = band_params.enabled.value();
        band.wet_mix = 1.0;
        band.bypass_fade.reset(1.0);
        band.band_type = Some(band_params.band_type.value());
        band.frequency_hz = band_params.frequency.value();
        band.gain_db = band_params.gain.value();
        band.effective_gain_db = band_params.gain.value();
        band.q = band_params.q.value();

        engine
    }

    fn test_signal(sample_count: usize) -> Vec<f32> {
        (0..sample_count)
            .map(|index| {
                let phase = std::f32::consts::TAU * 137.0 * index as f32 / SAMPLE_RATE;
                phase.sin() * 0.5 + (index as f32 * 0.013).sin() * 0.1
            })
            .collect()
    }

    fn dynamic_params(source: DynamicSource) -> EzeqParams {
        let params = EzeqParams::dynamic_test(source);
        let band = &params.band_2;
        band.frequency.smoothed.reset(band.frequency.value());
        band.q.smoothed.reset(band.q.value());
        band.dynamic_range
            .smoothed
            .reset(band.dynamic_range.value());
        band.dynamic_threshold
            .smoothed
            .reset(band.dynamic_threshold.value());
        params
    }

    #[test]
    fn bypass_fade_moves_smoothly_between_dry_and_wet() {
        let mut fade = LinearFade::default();
        let sample_rate = 1_000.0;

        fade.reset(0.0);
        let first_fade_in = fade.next(sample_rate, 1.0);
        assert!(first_fade_in > 0.0 && first_fade_in < 1.0);

        let mut wet_mix = first_fade_in;
        for _ in 1..BYPASS_FADE_MS as usize {
            wet_mix = fade.next(sample_rate, 1.0);
        }
        assert_eq!(wet_mix, 1.0);

        let first_fade_out = fade.next(sample_rate, 0.0);
        assert!(first_fade_out > 0.0 && first_fade_out < 1.0);
    }

    #[test]
    fn crossfade_preserves_both_endpoints() {
        assert_eq!(crossfade(-0.25, 0.75, 0.0), -0.25);
        assert_eq!(crossfade(-0.25, 0.75, 1.0), 0.75);
    }

    #[test]
    fn unusual_block_sizes_match_contiguous_processing() {
        const SAMPLE_COUNT: usize = 2_053;
        let params = EzeqParams::default();
        let input = test_signal(SAMPLE_COUNT);
        let mut contiguous = input.clone();
        let mut partitioned = input;
        let mut contiguous_engine = prepared_engine(64, &params);
        let mut partitioned_engine = prepared_engine(64, &params);

        contiguous_engine.process_channels(&mut [&mut contiguous], None, &params);

        let block_sizes = [1, 3, 17, 64, 257, 1_024];
        let mut offset = 0;
        let mut block_index = 0;
        while offset < SAMPLE_COUNT {
            let block_size =
                block_sizes[block_index % block_sizes.len()].min(SAMPLE_COUNT - offset);
            partitioned_engine.process_channels(
                &mut [&mut partitioned[offset..offset + block_size]],
                None,
                &params,
            );
            offset += block_size;
            block_index += 1;
        }

        assert_eq!(partitioned, contiguous);
    }

    #[test]
    fn mono_and_stereo_keep_identical_channels_isolated() {
        const SAMPLE_COUNT: usize = 257;
        let params = EzeqParams::default();
        let signal = test_signal(SAMPLE_COUNT);
        let mut mono = signal.clone();
        let mut stereo_left = signal;
        let mut stereo_right = vec![0.0; SAMPLE_COUNT];
        let mut mono_engine = prepared_engine(SAMPLE_COUNT, &params);
        let mut stereo_engine = prepared_engine(SAMPLE_COUNT, &params);

        mono_engine.process_channels(&mut [&mut mono], None, &params);
        stereo_engine.process_channels(&mut [&mut stereo_left, &mut stereo_right], None, &params);

        assert_eq!(stereo_left, mono);
        assert!(mono
            .iter()
            .zip(test_signal(SAMPLE_COUNT))
            .any(|(output, input)| output != &input));
        assert!(stereo_right.into_iter().all(|sample| sample == 0.0));
    }

    #[test]
    fn empty_blocks_and_zero_channel_buffers_are_safe() {
        let params = EzeqParams::default();
        let mut engine = prepared_engine(64, &params);
        let mut empty = [];

        engine.process_channels(&mut [], None, &params);
        engine.process_channels(&mut [&mut empty], None, &params);
    }

    #[test]
    fn continuous_automation_updates_coefficients_smoothly() {
        let params = EzeqParams::default();
        let mut engine = prepared_engine(64, &params);
        let mut samples = test_signal(64);

        params
            .band_1
            .frequency
            .smoothed
            .reset(params.band_1.frequency.value());
        params
            .band_1
            .gain
            .smoothed
            .reset(params.band_1.gain.value());
        params.band_1.q.smoothed.reset(params.band_1.q.value());
        params
            .band_1
            .frequency
            .smoothed
            .set_target(SAMPLE_RATE, 1_000.0);
        params.band_1.gain.smoothed.set_target(SAMPLE_RATE, 12.0);
        params.band_1.q.smoothed.set_target(SAMPLE_RATE, 2.0);
        engine.process_channels(&mut [&mut samples], None, &params);

        let band = &engine.bands[0];
        assert!(band.frequency_hz > 80.0 && band.frequency_hz < 1_000.0);
        assert!(band.gain_db > 0.0 && band.gain_db < 12.0);
        assert!(band.q > std::f32::consts::FRAC_1_SQRT_2 && band.q < 2.0);
        assert!(samples.into_iter().all(f32::is_finite));
    }

    #[test]
    fn external_sidechain_triggers_only_when_connected() {
        const SAMPLE_COUNT: usize = 48_000;
        let params = dynamic_params(DynamicSource::External);
        let mut engine = prepared_engine(SAMPLE_COUNT, &params);
        let mut main = vec![0.0; SAMPLE_COUNT];
        let mut sidechain: Vec<f32> = (0..SAMPLE_COUNT)
            .map(|index| (std::f32::consts::TAU * 1_000.0 * index as f32 / SAMPLE_RATE).sin())
            .collect();

        engine.process_channels(&mut [&mut main], Some(&[&mut sidechain]), &params);
        assert!(engine.bands[1].dynamic_gain_db < -3.0);
        assert!(main.into_iter().all(|sample| sample == 0.0));

        let disconnected_params = dynamic_params(DynamicSource::External);
        let mut disconnected_engine = prepared_engine(SAMPLE_COUNT, &disconnected_params);
        let mut disconnected_main = test_signal(SAMPLE_COUNT);
        disconnected_engine.process_channels(
            &mut [&mut disconnected_main],
            None,
            &disconnected_params,
        );
        assert_eq!(disconnected_engine.bands[1].dynamic_gain_db, -0.0);
    }

    #[test]
    fn internal_trigger_and_reset_clear_dynamic_state() {
        const SAMPLE_COUNT: usize = 48_000;
        let params = dynamic_params(DynamicSource::Internal);
        let mut engine = prepared_engine(SAMPLE_COUNT, &params);
        let mut main: Vec<f32> = (0..SAMPLE_COUNT)
            .map(|index| (std::f32::consts::TAU * 1_000.0 * index as f32 / SAMPLE_RATE).sin())
            .collect();

        engine.process_channels(&mut [&mut main], None, &params);
        assert!(engine.bands[1].dynamic_gain_db < -3.0);
        assert!(engine.dynamics_output.gain_db(1) < -3.0);

        engine.reset(&params);
        assert_eq!(engine.bands[1].dynamic_gain_db, 0.0);
        assert_eq!(engine.dynamics_output.gain_db(1), 0.0);
    }

    #[test]
    fn dynamic_response_curve_includes_live_gain_offsets() {
        let params = EzeqParams::dynamic_test(DynamicSource::Internal);
        let static_magnitude = response_magnitude(&params, SAMPLE_RATE, 1_000.0);
        let mut dynamic_gains = [0.0; NUM_BANDS];
        dynamic_gains[1] = -6.0;
        let dynamic_magnitude =
            response_magnitude_with_dynamic(&params, SAMPLE_RATE, 1_000.0, &dynamic_gains);

        assert!((static_magnitude - 1.0).abs() < 0.0001);
        assert!(dynamic_magnitude < static_magnitude);
    }
}
