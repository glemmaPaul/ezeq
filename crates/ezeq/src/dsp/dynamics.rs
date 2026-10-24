//! Real-time-safe dynamic EQ detection and gain control.

use super::biquad::{Biquad, BiquadCoefficients, CoefficientGenerator};
use crate::params::{DynamicSource, EqBandParams, EqBandType, NUM_BANDS};
use std::sync::atomic::{AtomicU32, Ordering};

const MAX_CHANNELS: usize = 2;
const SOFT_KNEE_DB: f32 = 6.0;
const FIXED_RATIO_SLOPE: f32 = 0.5;
const MIN_DETECTOR_AMPLITUDE: f32 = 1.0e-9;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TriggerFrame {
    pub(super) samples: [f32; MAX_CHANNELS],
    pub(super) channels: usize,
}

impl TriggerFrame {
    pub(super) fn from_channels(channels: &[&mut [f32]], sample_index: usize) -> Self {
        let mut frame = Self {
            channels: channels.len().min(MAX_CHANNELS),
            ..Self::default()
        };
        for (channel_index, channel) in channels.iter().take(MAX_CHANNELS).enumerate() {
            frame.samples[channel_index] = channel.get(sample_index).copied().unwrap_or(0.0);
        }
        frame
    }
}

/// Lock-free audio-thread-to-editor dynamic gain metering.
pub(crate) struct DynamicsOutput {
    gain_db_bits: [AtomicU32; NUM_BANDS],
}

impl Default for DynamicsOutput {
    fn default() -> Self {
        Self {
            gain_db_bits: std::array::from_fn(|_| AtomicU32::new(0.0_f32.to_bits())),
        }
    }
}

impl DynamicsOutput {
    pub(crate) fn gain_db(&self, band_index: usize) -> f32 {
        f32::from_bits(self.gain_db_bits[band_index].load(Ordering::Relaxed))
    }

    pub(crate) fn snapshot(&self) -> [f32; NUM_BANDS] {
        std::array::from_fn(|index| self.gain_db(index))
    }

    pub(super) fn set_gain_db(&self, band_index: usize, gain_db: f32) {
        self.gain_db_bits[band_index].store(gain_db.to_bits(), Ordering::Relaxed);
    }

    pub(super) fn reset(&self) {
        for gain in &self.gain_db_bits {
            gain.store(0.0_f32.to_bits(), Ordering::Relaxed);
        }
    }
}

pub(super) struct DynamicBand {
    detectors: [Biquad; MAX_CHANNELS],
    envelope: EnvelopeFollower,
    source: Option<DynamicSource>,
    band_type: Option<EqBandType>,
    frequency_hz: f32,
    q: f32,
    enabled: bool,
    gain_db: f32,
}

impl Default for DynamicBand {
    fn default() -> Self {
        Self {
            detectors: std::array::from_fn(|_| Biquad::default()),
            envelope: EnvelopeFollower::default(),
            source: None,
            band_type: None,
            frequency_hz: f32::NAN,
            q: f32::NAN,
            enabled: false,
            gain_db: 0.0,
        }
    }
}

impl DynamicBand {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn next(
        &mut self,
        generator: CoefficientGenerator,
        params: &EqBandParams,
        band_type: EqBandType,
        frequency_hz: f32,
        q: f32,
        sample_rate: f32,
        internal: TriggerFrame,
        external: TriggerFrame,
    ) -> f32 {
        // Advance smoothers while disabled so re-enabling never resumes an old ramp.
        let range_db = params.dynamic_range.smoothed.next();
        let threshold_db = params.dynamic_threshold.smoothed.next();
        let source = params.dynamic_source.value();
        let enabled =
            params.enabled.value() && params.dynamic_enabled.value() && band_type.uses_gain();

        let topology_changed =
            self.band_type != Some(band_type) || frequency_hz != self.frequency_hz || q != self.q;
        let source_changed = self.source != Some(source);
        let enable_changed = self.enabled != enabled;

        if topology_changed {
            let coefficients = detector_coefficients(generator, band_type, frequency_hz, q);
            for detector in &mut self.detectors {
                detector.set_coefficients(coefficients);
            }
        }

        if source_changed || enable_changed {
            self.reset_state();
        }

        self.source = Some(source);
        self.band_type = Some(band_type);
        self.frequency_hz = frequency_hz;
        self.q = q;
        self.enabled = enabled;

        if !enabled {
            self.gain_db = 0.0;
            return 0.0;
        }

        let trigger = match source {
            DynamicSource::Internal => internal,
            DynamicSource::External => external,
        };
        let mut detected_peak = 0.0_f32;
        for channel_index in 0..trigger.channels.min(MAX_CHANNELS) {
            let filtered = self.detectors[channel_index]
                .process_sample(trigger.samples[channel_index])
                .abs();
            detected_peak = detected_peak.max(filtered);
        }

        let envelope = self.envelope.next(
            detected_peak,
            params.dynamic_attack.value(),
            params.dynamic_release.value(),
            sample_rate,
        );
        let level_db = 20.0 * envelope.max(MIN_DETECTOR_AMPLITUDE).log10();
        self.gain_db = dynamic_gain_db(level_db, threshold_db, range_db);
        self.gain_db
    }

    pub(super) fn reset(&mut self) {
        self.reset_state();
        self.source = None;
        self.band_type = None;
        self.frequency_hz = f32::NAN;
        self.q = f32::NAN;
        self.enabled = false;
    }

    fn reset_state(&mut self) {
        for detector in &mut self.detectors {
            detector.reset();
        }
        self.envelope.reset();
        self.gain_db = 0.0;
    }
}

#[derive(Default)]
struct EnvelopeFollower {
    envelope: f32,
}

impl EnvelopeFollower {
    #[inline]
    fn next(&mut self, input: f32, attack_ms: f32, release_ms: f32, sample_rate: f32) -> f32 {
        let time_ms = if input > self.envelope {
            attack_ms
        } else {
            release_ms
        };
        let coefficient = time_coefficient(time_ms, sample_rate);
        self.envelope = input + coefficient * (self.envelope - input);
        if self.envelope.abs() < MIN_DETECTOR_AMPLITUDE {
            self.envelope = 0.0;
        }
        self.envelope
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
    }
}

fn detector_coefficients(
    generator: CoefficientGenerator,
    band_type: EqBandType,
    frequency_hz: f32,
    q: f32,
) -> BiquadCoefficients {
    match band_type {
        EqBandType::LowShelf => generator.low_pass(frequency_hz, q),
        EqBandType::Bell => generator.band_pass(frequency_hz, q),
        EqBandType::HighShelf => generator.high_pass(frequency_hz, q),
        EqBandType::HighPass | EqBandType::LowPass => BiquadCoefficients::IDENTITY,
    }
}

fn dynamic_gain_db(level_db: f32, threshold_db: f32, range_db: f32) -> f32 {
    let half_knee = SOFT_KNEE_DB * 0.5;
    let distance = level_db - threshold_db;
    let over_db = if distance <= -half_knee {
        0.0
    } else if distance >= half_knee {
        distance
    } else {
        let knee_position = distance + half_knee;
        knee_position * knee_position / (2.0 * SOFT_KNEE_DB)
    };
    let magnitude = (over_db * FIXED_RATIO_SLOPE).min(range_db.abs());
    magnitude.copysign(range_db)
}

fn time_coefficient(time_ms: f32, sample_rate: f32) -> f32 {
    let seconds = (time_ms.max(0.001) * 0.001).max(f32::MIN_POSITIVE);
    (-1.0 / (seconds * sample_rate.max(1.0))).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;

    #[test]
    fn gain_curve_is_thresholded_and_range_limited() {
        assert_eq!(dynamic_gain_db(-40.0, -24.0, -6.0), -0.0);
        assert!((dynamic_gain_db(-12.0, -24.0, -6.0) + 6.0).abs() < 0.0001);
        assert!((dynamic_gain_db(-12.0, -24.0, 4.0) - 4.0).abs() < 0.0001);
    }

    #[test]
    fn envelope_uses_attack_and_release() {
        let mut follower = EnvelopeFollower::default();
        let attack = follower.next(1.0, 10.0, 100.0, SAMPLE_RATE);
        assert!(attack > 0.0 && attack < 1.0);

        for _ in 0..4_800 {
            follower.next(1.0, 10.0, 100.0, SAMPLE_RATE);
        }
        let release = follower.next(0.0, 10.0, 100.0, SAMPLE_RATE);
        assert!(release > attack && release < 1.0);
    }

    #[test]
    fn bell_detector_is_frequency_selective() {
        let generator = CoefficientGenerator::new(SAMPLE_RATE);
        let detector = detector_coefficients(generator, EqBandType::Bell, 1_000.0, 2.0);

        assert!((detector.magnitude_at(SAMPLE_RATE, 1_000.0) - 1.0).abs() < 0.0001);
        assert!(detector.magnitude_at(SAMPLE_RATE, 100.0) < 0.1);
        assert!(detector.magnitude_at(SAMPLE_RATE, 10_000.0) < 0.1);
    }

    #[test]
    fn meter_snapshot_round_trips_signed_gain() {
        let output = DynamicsOutput::default();
        output.set_gain_db(2, -4.5);
        output.set_gain_db(5, 3.25);

        let snapshot = output.snapshot();
        assert_eq!(snapshot[2], -4.5);
        assert_eq!(snapshot[5], 3.25);
    }
}
