//! Allocation-free biquad filtering and RBJ-style coefficient generation.
#![allow(dead_code)]

use std::f32::consts::TAU;

const MIN_Q: f32 = 0.0001;
const MIN_SHELF_SLOPE: f32 = 0.0001;
const MAX_GAIN_DB: f32 = 120.0;

/// Normalized coefficients for
/// `y[n] = b0*x[n] + b1*x[n-1] + b2*x[n-2] - a1*y[n-1] - a2*y[n-2]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BiquadCoefficients {
    pub(super) b0: f32,
    pub(super) b1: f32,
    pub(super) b2: f32,
    pub(super) a1: f32,
    pub(super) a2: f32,
}

impl BiquadCoefficients {
    pub(super) const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn normalized(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        let inverse_a0 = a0.recip();
        Self {
            b0: b0 * inverse_a0,
            b1: b1 * inverse_a0,
            b2: b2 * inverse_a0,
            a1: a1 * inverse_a0,
            a2: a2 * inverse_a0,
        }
    }

    pub(super) fn magnitude_at(self, sample_rate: f32, frequency_hz: f32) -> f32 {
        let omega = TAU * frequency_hz / sample_rate;
        let (sin_omega, cos_omega) = omega.sin_cos();
        let sin_two_omega = (2.0 * omega).sin();
        let cos_two_omega = (2.0 * omega).cos();

        let numerator_re = self.b0 + self.b1 * cos_omega + self.b2 * cos_two_omega;
        let numerator_im = -self.b1 * sin_omega - self.b2 * sin_two_omega;
        let denominator_re = 1.0 + self.a1 * cos_omega + self.a2 * cos_two_omega;
        let denominator_im = -self.a1 * sin_omega - self.a2 * sin_two_omega;

        ((numerator_re * numerator_re + numerator_im * numerator_im)
            / (denominator_re * denominator_re + denominator_im * denominator_im))
            .sqrt()
    }
}

/// Generates normalized coefficients for a fixed sample rate.
#[derive(Clone, Copy, Debug)]
pub(super) struct CoefficientGenerator {
    sample_rate: f32,
}

impl CoefficientGenerator {
    pub(super) fn new(sample_rate: f32) -> Self {
        assert!(
            sample_rate.is_finite() && sample_rate > 0.0,
            "sample rate must be finite and positive"
        );
        Self { sample_rate }
    }

    pub(super) fn low_pass(self, frequency_hz: f32, q: f32) -> BiquadCoefficients {
        let (sin_omega, cos_omega) = self.sin_cos(frequency_hz);
        let alpha = sin_omega / (2.0 * sanitize_q(q));

        BiquadCoefficients::normalized(
            (1.0 - cos_omega) * 0.5,
            1.0 - cos_omega,
            (1.0 - cos_omega) * 0.5,
            1.0 + alpha,
            -2.0 * cos_omega,
            1.0 - alpha,
        )
    }

    pub(super) fn high_pass(self, frequency_hz: f32, q: f32) -> BiquadCoefficients {
        let (sin_omega, cos_omega) = self.sin_cos(frequency_hz);
        let alpha = sin_omega / (2.0 * sanitize_q(q));

        BiquadCoefficients::normalized(
            (1.0 + cos_omega) * 0.5,
            -(1.0 + cos_omega),
            (1.0 + cos_omega) * 0.5,
            1.0 + alpha,
            -2.0 * cos_omega,
            1.0 - alpha,
        )
    }

    pub(super) fn peaking(self, frequency_hz: f32, q: f32, gain_db: f32) -> BiquadCoefficients {
        let (sin_omega, cos_omega) = self.sin_cos(frequency_hz);
        let alpha = sin_omega / (2.0 * sanitize_q(q));
        let amplitude = db_to_sqrt_amplitude(gain_db);

        BiquadCoefficients::normalized(
            1.0 + alpha * amplitude,
            -2.0 * cos_omega,
            1.0 - alpha * amplitude,
            1.0 + alpha / amplitude,
            -2.0 * cos_omega,
            1.0 - alpha / amplitude,
        )
    }

    /// Generates low-shelf coefficients.
    ///
    /// `slope` follows the Audio EQ Cookbook definition and is clamped to
    /// `(0, 1]`, where `1` is the steepest monotonic shelf.
    pub(super) fn low_shelf(
        self,
        frequency_hz: f32,
        slope: f32,
        gain_db: f32,
    ) -> BiquadCoefficients {
        let (sin_omega, cos_omega) = self.sin_cos(frequency_hz);
        let amplitude = db_to_sqrt_amplitude(gain_db);
        let alpha = shelf_alpha(sin_omega, amplitude, slope);
        let two_sqrt_a_alpha = 2.0 * amplitude.sqrt() * alpha;
        let a_plus_one = amplitude + 1.0;
        let a_minus_one = amplitude - 1.0;

        BiquadCoefficients::normalized(
            amplitude * (a_plus_one - a_minus_one * cos_omega + two_sqrt_a_alpha),
            2.0 * amplitude * (a_minus_one - a_plus_one * cos_omega),
            amplitude * (a_plus_one - a_minus_one * cos_omega - two_sqrt_a_alpha),
            a_plus_one + a_minus_one * cos_omega + two_sqrt_a_alpha,
            -2.0 * (a_minus_one + a_plus_one * cos_omega),
            a_plus_one + a_minus_one * cos_omega - two_sqrt_a_alpha,
        )
    }

    /// Generates high-shelf coefficients.
    ///
    /// `slope` follows the Audio EQ Cookbook definition and is clamped to
    /// `(0, 1]`, where `1` is the steepest monotonic shelf.
    pub(super) fn high_shelf(
        self,
        frequency_hz: f32,
        slope: f32,
        gain_db: f32,
    ) -> BiquadCoefficients {
        let (sin_omega, cos_omega) = self.sin_cos(frequency_hz);
        let amplitude = db_to_sqrt_amplitude(gain_db);
        let alpha = shelf_alpha(sin_omega, amplitude, slope);
        let two_sqrt_a_alpha = 2.0 * amplitude.sqrt() * alpha;
        let a_plus_one = amplitude + 1.0;
        let a_minus_one = amplitude - 1.0;

        BiquadCoefficients::normalized(
            amplitude * (a_plus_one + a_minus_one * cos_omega + two_sqrt_a_alpha),
            -2.0 * amplitude * (a_minus_one + a_plus_one * cos_omega),
            amplitude * (a_plus_one + a_minus_one * cos_omega - two_sqrt_a_alpha),
            a_plus_one - a_minus_one * cos_omega + two_sqrt_a_alpha,
            2.0 * (a_minus_one - a_plus_one * cos_omega),
            a_plus_one - a_minus_one * cos_omega - two_sqrt_a_alpha,
        )
    }

    fn sin_cos(self, frequency_hz: f32) -> (f32, f32) {
        let nyquist = self.sample_rate * 0.5;
        let maximum = nyquist * (1.0 - f32::EPSILON);
        let frequency_hz = if frequency_hz.is_finite() {
            frequency_hz.clamp(0.0, maximum)
        } else {
            0.0
        };

        (TAU * frequency_hz / self.sample_rate).sin_cos()
    }
}

/// A transposed-direct-form-II biquad with two state values.
#[derive(Clone, Copy, Debug)]
pub(super) struct Biquad {
    coefficients: BiquadCoefficients,
    state_1: f32,
    state_2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self::new(BiquadCoefficients::IDENTITY)
    }
}

impl Biquad {
    pub(super) const fn new(coefficients: BiquadCoefficients) -> Self {
        Self {
            coefficients,
            state_1: 0.0,
            state_2: 0.0,
        }
    }

    /// Replaces coefficients without resetting delay state.
    pub(super) fn set_coefficients(&mut self, coefficients: BiquadCoefficients) {
        self.coefficients = coefficients;
    }

    pub(super) fn reset(&mut self) {
        self.state_1 = 0.0;
        self.state_2 = 0.0;
    }

    #[inline]
    pub(super) fn process_sample(&mut self, input: f32) -> f32 {
        let output = self.coefficients.b0 * input + self.state_1;
        self.state_1 = zap_denormal(
            self.coefficients.b1 * input - self.coefficients.a1 * output + self.state_2,
        );
        self.state_2 = zap_denormal(self.coefficients.b2 * input - self.coefficients.a2 * output);
        output
    }

    pub(super) fn process_block(&mut self, samples: &mut [f32]) {
        for sample in samples {
            *sample = self.process_sample(*sample);
        }
    }
}

fn sanitize_q(q: f32) -> f32 {
    if q.is_finite() {
        q.max(MIN_Q)
    } else {
        MIN_Q
    }
}

fn sanitize_slope(slope: f32) -> f32 {
    if slope.is_finite() {
        slope.clamp(MIN_SHELF_SLOPE, 1.0)
    } else {
        1.0
    }
}

fn db_to_sqrt_amplitude(gain_db: f32) -> f32 {
    let gain_db = if gain_db.is_finite() {
        gain_db.clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
    } else {
        0.0
    };
    10.0_f32.powf(gain_db / 40.0)
}

fn shelf_alpha(sin_omega: f32, amplitude: f32, slope: f32) -> f32 {
    let slope = sanitize_slope(slope);
    let radicand = (amplitude + amplitude.recip()) * (slope.recip() - 1.0) + 2.0;
    sin_omega * 0.5 * radicand.sqrt()
}

#[inline]
fn zap_denormal(value: f32) -> f32 {
    if value.abs() < 1.0e-30 {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;
    const TEST_FREQUENCY: f32 = 1_000.0;
    const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

    fn assert_close(actual: f32, expected: f32, tolerance: f32) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn identity_is_sample_exact() {
        let mut biquad = Biquad::default();
        let expected = [-1.0, -0.25, 0.0, 0.5, 1.0];
        let mut actual = expected;

        biquad.process_block(&mut actual);

        assert_eq!(actual, expected);
    }

    #[test]
    fn reset_clears_filter_state() {
        let coefficients =
            CoefficientGenerator::new(SAMPLE_RATE).low_pass(TEST_FREQUENCY, BUTTERWORTH_Q);
        let mut biquad = Biquad::new(coefficients);

        let _ = biquad.process_sample(1.0);
        biquad.reset();

        assert_eq!(biquad.process_sample(0.0), 0.0);
    }

    #[test]
    fn low_pass_has_unity_dc_gain_and_rejects_nyquist() {
        let coefficients =
            CoefficientGenerator::new(SAMPLE_RATE).low_pass(TEST_FREQUENCY, BUTTERWORTH_Q);

        assert_close(coefficients.magnitude_at(SAMPLE_RATE, 0.0), 1.0, 0.0001);
        assert!(coefficients.magnitude_at(SAMPLE_RATE, 23_900.0) < 0.0001);
    }

    #[test]
    fn high_pass_rejects_dc_and_has_unity_high_frequency_gain() {
        let coefficients =
            CoefficientGenerator::new(SAMPLE_RATE).high_pass(TEST_FREQUENCY, BUTTERWORTH_Q);

        assert!(coefficients.magnitude_at(SAMPLE_RATE, 1.0) < 0.0001);
        assert_close(
            coefficients.magnitude_at(SAMPLE_RATE, 23_900.0),
            1.0,
            0.0001,
        );
    }

    #[test]
    fn peaking_filter_reaches_requested_center_gain() {
        let coefficients = CoefficientGenerator::new(SAMPLE_RATE).peaking(TEST_FREQUENCY, 1.0, 6.0);
        let expected_gain = 10.0_f32.powf(6.0 / 20.0);

        assert_close(
            coefficients.magnitude_at(SAMPLE_RATE, TEST_FREQUENCY),
            expected_gain,
            0.0001,
        );
        assert_close(coefficients.magnitude_at(SAMPLE_RATE, 0.0), 1.0, 0.0001);
    }

    #[test]
    fn shelves_reach_requested_gain_on_the_correct_side() {
        let generator = CoefficientGenerator::new(SAMPLE_RATE);
        let expected_gain = 10.0_f32.powf(6.0 / 20.0);
        let low_shelf = generator.low_shelf(TEST_FREQUENCY, 1.0, 6.0);
        let high_shelf = generator.high_shelf(TEST_FREQUENCY, 1.0, 6.0);

        assert_close(
            low_shelf.magnitude_at(SAMPLE_RATE, 0.0),
            expected_gain,
            0.0001,
        );
        assert_close(low_shelf.magnitude_at(SAMPLE_RATE, 23_900.0), 1.0, 0.0001);
        assert_close(high_shelf.magnitude_at(SAMPLE_RATE, 0.0), 1.0, 0.0001);
        assert_close(
            high_shelf.magnitude_at(SAMPLE_RATE, 23_900.0),
            expected_gain,
            0.0001,
        );
    }

    #[test]
    fn biquad_applies_peaking_gain_to_a_sine_wave() {
        let coefficients = CoefficientGenerator::new(SAMPLE_RATE).peaking(TEST_FREQUENCY, 1.0, 6.0);
        let mut biquad = Biquad::new(coefficients);
        let mut input_energy = 0.0_f64;
        let mut output_energy = 0.0_f64;

        for index in 0..48_000 {
            let input = (TAU * TEST_FREQUENCY * index as f32 / SAMPLE_RATE).sin();
            let output = biquad.process_sample(input);
            if index >= 24_000 {
                input_energy += f64::from(input * input);
                output_energy += f64::from(output * output);
            }
        }

        let measured_gain = (output_energy / input_energy).sqrt() as f32;
        assert_close(measured_gain, 10.0_f32.powf(6.0 / 20.0), 0.001);
    }

    #[test]
    fn invalid_parameter_values_are_sanitized_to_finite_coefficients() {
        let generator = CoefficientGenerator::new(SAMPLE_RATE);
        let coefficient_sets = [
            generator.low_pass(f32::NAN, f32::NAN),
            generator.high_pass(f32::INFINITY, -1.0),
            generator.peaking(TEST_FREQUENCY, 0.0, f32::INFINITY),
            generator.low_shelf(TEST_FREQUENCY, f32::NAN, f32::NAN),
            generator.high_shelf(TEST_FREQUENCY, -1.0, f32::NEG_INFINITY),
        ];

        for coefficients in coefficient_sets {
            assert!([
                coefficients.b0,
                coefficients.b1,
                coefficients.b2,
                coefficients.a1,
                coefficients.a2,
            ]
            .into_iter()
            .all(f32::is_finite));
        }
    }
}
