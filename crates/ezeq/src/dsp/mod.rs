mod biquad;
mod spectrum;

pub(crate) use spectrum::{spectrum_channel, SpectrumOutput, MIN_DB, SPECTRUM_BINS};

use self::biquad::{Biquad, BiquadCoefficients, CoefficientGenerator};
use self::spectrum::SpectrumInput;
use crate::params::{EqBandParams, EqBandType, EzeqParams, NUM_BANDS};
use nih_plug::prelude::{Buffer, Smoother, SmoothingStyle};

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
}

impl EqEngine {
    pub(crate) fn new(spectrum_input: SpectrumInput) -> Self {
        Self {
            sample_rate: 0.0,
            max_buffer_size: 0,
            bands: std::array::from_fn(|_| EqBand::default()),
            global_bypass_fade: LinearFade::default(),
            spectrum_input,
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
    }

    #[inline]
    pub(crate) fn process(&mut self, buffer: &mut Buffer, params: &EzeqParams) {
        debug_assert!(self.sample_rate > 0.0);
        debug_assert!(self.max_buffer_size > 0);

        let channel_count = buffer.channels();
        if channel_count == 0 {
            return;
        }
        debug_assert!(channel_count <= MAX_CHANNELS);

        let channel_gain = 1.0 / channel_count as f32;
        let generator = CoefficientGenerator::new(self.sample_rate);

        for mut channel_samples in buffer.iter_samples() {
            let global_dry_mix = self
                .global_bypass_fade
                .next(self.sample_rate, bool_to_mix(params.bypass.value()));
            let mono_sample = channel_samples
                .iter_mut()
                .map(|sample| *sample)
                .sum::<f32>()
                * channel_gain;
            self.spectrum_input.push_sample(mono_sample);

            for (band, band_params) in self.bands.iter_mut().zip(params.bands()) {
                band.update(generator, band_params, self.sample_rate);
            }

            for (channel_index, sample) in channel_samples.iter_mut().enumerate() {
                if channel_index >= MAX_CHANNELS {
                    break;
                }

                let dry = *sample;
                let mut wet = dry;
                for band in &mut self.bands {
                    wet = band.process_sample(channel_index, wet);
                }
                *sample = crossfade(wet, dry, global_dry_mix);
            }
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
    q: f32,
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
            q: f32::NAN,
        }
    }
}

impl EqBand {
    #[inline]
    fn update(&mut self, generator: CoefficientGenerator, params: &EqBandParams, sample_rate: f32) {
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

        if should_process
            && (topology_changed
                || enable_changed
                || frequency_hz != self.frequency_hz
                || gain_db != self.gain_db
                || q != self.q)
        {
            let coefficients =
                coefficients_for_band(generator, band_type, frequency_hz, gain_db, q);
            for filter in &mut self.filters {
                filter.set_coefficients(coefficients);
            }
        }

        self.enabled = enabled;
        self.band_type = Some(band_type);
        self.frequency_hz = frequency_hz;
        self.gain_db = gain_db;
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
        self.enabled = enabled;
        self.wet_mix = bool_to_mix(enabled);
        self.bypass_fade.reset(self.wet_mix);
    }

    fn invalidate_coefficients(&mut self) {
        self.reset_filter_state();
        self.frequency_hz = f32::NAN;
        self.gain_db = f32::NAN;
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
    if params.bypass.value() {
        return 1.0;
    }

    let generator = CoefficientGenerator::new(sample_rate);
    params
        .bands()
        .into_iter()
        .filter(|band| band.enabled.value())
        .map(|band| {
            coefficients_for_band(
                generator,
                band.band_type.value(),
                band.frequency.value(),
                band.gain.value(),
                band.q.value(),
            )
            .magnitude_at(sample_rate, frequency_hz)
        })
        .product()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
