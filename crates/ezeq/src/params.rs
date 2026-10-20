use nih_plug::prelude::{
    formatters, BoolParam, Enum, EnumParam, FloatParam, FloatRange, Params, SmoothingStyle,
};
use nih_plug_egui::EguiState;
use std::sync::Arc;

pub(crate) const NUM_BANDS: usize = 8;

const MIN_FREQUENCY_HZ: f32 = 20.0;
const MAX_FREQUENCY_HZ: f32 = 20_000.0;
const MIN_GAIN_DB: f32 = -24.0;
const MAX_GAIN_DB: f32 = 24.0;
const MIN_Q: f32 = 0.1;
const MAX_Q: f32 = 18.0;
const DEFAULT_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;
const PARAMETER_SMOOTHING_MS: f32 = 20.0;

/// Host-visible and persisted plugin state.
///
/// Every `id` and nested `id_prefix` below is a session compatibility contract.
/// Never rename or reuse one after release. The explicit per-band prefixes make
/// IDs independent of field order and allow more bands to be added later.
#[derive(Params)]
pub struct EzeqParams {
    #[persist = "editor-state"]
    pub(crate) editor_state: Arc<EguiState>,

    /// Linked to the host's native bypass control when the format supports it.
    #[id = "bypass"]
    pub(crate) bypass: BoolParam,

    #[nested(id_prefix = "b1", group = "Band 1")]
    pub(crate) band_1: EqBandParams,
    #[nested(id_prefix = "b2", group = "Band 2")]
    pub(crate) band_2: EqBandParams,
    #[nested(id_prefix = "b3", group = "Band 3")]
    pub(crate) band_3: EqBandParams,
    #[nested(id_prefix = "b4", group = "Band 4")]
    pub(crate) band_4: EqBandParams,
    #[nested(id_prefix = "b5", group = "Band 5")]
    pub(crate) band_5: EqBandParams,
    #[nested(id_prefix = "b6", group = "Band 6")]
    pub(crate) band_6: EqBandParams,
    #[nested(id_prefix = "b7", group = "Band 7")]
    pub(crate) band_7: EqBandParams,
    #[nested(id_prefix = "b8", group = "Band 8")]
    pub(crate) band_8: EqBandParams,
}

impl Default for EzeqParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(900, 560),
            bypass: BoolParam::new("Bypass", false)
                .with_value_to_string(formatters::v2s_bool_bypass())
                .with_string_to_value(formatters::s2v_bool_bypass())
                .make_bypass(),
            band_1: EqBandParams::new(EqBandType::LowShelf, 80.0),
            band_2: EqBandParams::new(EqBandType::Bell, 160.0),
            band_3: EqBandParams::new(EqBandType::Bell, 320.0),
            band_4: EqBandParams::new(EqBandType::Bell, 640.0),
            band_5: EqBandParams::new(EqBandType::Bell, 1_250.0),
            band_6: EqBandParams::new(EqBandType::Bell, 2_500.0),
            band_7: EqBandParams::new(EqBandType::Bell, 5_000.0),
            band_8: EqBandParams::new(EqBandType::HighShelf, 10_000.0),
        }
    }
}

impl EzeqParams {
    pub(crate) fn bands(&self) -> [&EqBandParams; NUM_BANDS] {
        [
            &self.band_1,
            &self.band_2,
            &self.band_3,
            &self.band_4,
            &self.band_5,
            &self.band_6,
            &self.band_7,
            &self.band_8,
        ]
    }
}

#[derive(Params)]
pub(crate) struct EqBandParams {
    #[id = "on"]
    pub(crate) enabled: BoolParam,

    #[id = "type"]
    pub(crate) band_type: EnumParam<EqBandType>,

    #[id = "freq"]
    pub(crate) frequency: FloatParam,

    #[id = "gain"]
    pub(crate) gain: FloatParam,

    #[id = "q"]
    pub(crate) q: FloatParam,
}

impl EqBandParams {
    fn new(band_type: EqBandType, frequency_hz: f32) -> Self {
        Self {
            enabled: BoolParam::new("Enabled", false),
            band_type: EnumParam::new("Type", band_type),
            frequency: FloatParam::new(
                "Frequency",
                frequency_hz,
                FloatRange::Skewed {
                    min: MIN_FREQUENCY_HZ,
                    max: MAX_FREQUENCY_HZ,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(PARAMETER_SMOOTHING_MS))
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(1))
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),
            gain: FloatParam::new(
                "Gain",
                0.0,
                FloatRange::Linear {
                    min: MIN_GAIN_DB,
                    max: MAX_GAIN_DB,
                },
            )
            // A linear ramp in decibels is a smooth multiplicative change in amplitude.
            .with_smoother(SmoothingStyle::Linear(PARAMETER_SMOOTHING_MS))
            .with_unit(" dB")
            .with_step_size(0.1)
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            q: FloatParam::new(
                "Q",
                DEFAULT_Q,
                FloatRange::Skewed {
                    min: MIN_Q,
                    max: MAX_Q,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(PARAMETER_SMOOTHING_MS))
            .with_step_size(0.01)
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
        }
    }
}

/// Available biquad responses.
///
/// The variant IDs are serialized by NIH-plug. Keep the IDs and their meaning
/// stable even if display names or Rust variant names change.
#[derive(Clone, Copy, Debug, Eq, Enum, PartialEq)]
pub(crate) enum EqBandType {
    #[id = "high-pass"]
    #[name = "High Pass"]
    HighPass,

    #[id = "low-shelf"]
    #[name = "Low Shelf"]
    LowShelf,

    #[id = "bell"]
    Bell,

    #[id = "high-shelf"]
    #[name = "High Shelf"]
    HighShelf,

    #[id = "low-pass"]
    #[name = "Low Pass"]
    LowPass,
}

#[cfg(test)]
mod tests {
    use super::*;
    use nih_plug::prelude::{Param, ParamFlags};

    #[test]
    fn parameter_ids_are_a_permanent_compatibility_contract() {
        let ids: Vec<String> = EzeqParams::default()
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();

        assert_eq!(
            ids,
            [
                "bypass", "b1_on", "b1_type", "b1_freq", "b1_gain", "b1_q", "b2_on", "b2_type",
                "b2_freq", "b2_gain", "b2_q", "b3_on", "b3_type", "b3_freq", "b3_gain", "b3_q",
                "b4_on", "b4_type", "b4_freq", "b4_gain", "b4_q", "b5_on", "b5_type", "b5_freq",
                "b5_gain", "b5_q", "b6_on", "b6_type", "b6_freq", "b6_gain", "b6_q", "b7_on",
                "b7_type", "b7_freq", "b7_gain", "b7_q", "b8_on", "b8_type", "b8_freq", "b8_gain",
                "b8_q",
            ]
        );
    }

    #[test]
    fn default_bands_are_disabled_and_cover_the_audible_spectrum() {
        let params = EzeqParams::default();
        let bands = [
            &params.band_1,
            &params.band_2,
            &params.band_3,
            &params.band_4,
            &params.band_5,
            &params.band_6,
            &params.band_7,
            &params.band_8,
        ];
        let expected_frequencies = [
            80.0, 160.0, 320.0, 640.0, 1_250.0, 2_500.0, 5_000.0, 10_000.0,
        ];

        for (band, expected_frequency) in bands.into_iter().zip(expected_frequencies) {
            assert!(!band.enabled.value());
            assert_eq!(band.frequency.value(), expected_frequency);
            assert_eq!(band.gain.value(), 0.0);
            assert_eq!(band.q.value(), DEFAULT_Q);
        }

        assert_eq!(params.band_1.band_type.value(), EqBandType::LowShelf);
        assert_eq!(params.band_8.band_type.value(), EqBandType::HighShelf);
        assert!(!params.bypass.value());
        assert!(params.bypass.flags().contains(ParamFlags::BYPASS));
    }

    #[test]
    fn band_type_serialization_ids_are_stable() {
        assert_eq!(
            EqBandType::ids(),
            Some(&["high-pass", "low-shelf", "bell", "high-shelf", "low-pass",][..])
        );
    }

    #[test]
    fn continuous_band_parameters_have_short_safe_smoothing_ramps() {
        let params = EzeqParams::default();

        assert!(matches!(
            &params.band_1.frequency.smoothed.style,
            SmoothingStyle::Logarithmic(milliseconds)
                if *milliseconds == PARAMETER_SMOOTHING_MS
        ));
        assert!(matches!(
            &params.band_1.gain.smoothed.style,
            SmoothingStyle::Linear(milliseconds) if *milliseconds == PARAMETER_SMOOTHING_MS
        ));
        assert!(matches!(
            &params.band_1.q.smoothed.style,
            SmoothingStyle::Logarithmic(milliseconds)
                if *milliseconds == PARAMETER_SMOOTHING_MS
        ));
    }

    #[test]
    fn smoothers_move_gradually_and_reach_their_targets() {
        let params = EzeqParams::default();
        let sample_rate = 1_000.0;
        let steps = PARAMETER_SMOOTHING_MS as usize;

        params.band_1.frequency.smoothed.reset(100.0);
        params
            .band_1
            .frequency
            .smoothed
            .set_target(sample_rate, 1_000.0);
        let first_frequency = params.band_1.frequency.smoothed.next();
        assert!(first_frequency > 100.0 && first_frequency < 1_000.0);
        for _ in 1..steps {
            params.band_1.frequency.smoothed.next();
        }
        assert_eq!(params.band_1.frequency.smoothed.next(), 1_000.0);

        params.band_1.gain.smoothed.reset(-12.0);
        params.band_1.gain.smoothed.set_target(sample_rate, 12.0);
        let first_gain = params.band_1.gain.smoothed.next();
        assert!(first_gain > -12.0 && first_gain < 12.0);
        for _ in 1..steps {
            params.band_1.gain.smoothed.next();
        }
        assert_eq!(params.band_1.gain.smoothed.next(), 12.0);
    }
}
