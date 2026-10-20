use std::f32::consts::TAU;
use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering},
    Arc,
};

pub(crate) const SPECTRUM_BINS: usize = 256;
pub(crate) const MIN_DB: f32 = -90.0;
const FFT_SIZE: usize = 2048;
const MIN_FREQUENCY: f32 = 20.0;
const MAX_FREQUENCY: f32 = 20_000.0;

/// Lock-free spectrum data shared by the audio thread and editor.
pub(crate) struct SpectrumOutput {
    bins: [AtomicU32; SPECTRUM_BINS],
    generation: AtomicU64,
}

impl Default for SpectrumOutput {
    fn default() -> Self {
        Self {
            bins: std::array::from_fn(|_| AtomicU32::new(MIN_DB.to_bits())),
            generation: AtomicU64::new(0),
        }
    }
}

impl SpectrumOutput {
    pub(crate) fn read(&self, destination: &mut [f32; SPECTRUM_BINS]) {
        let _ = self.generation.load(Ordering::Acquire);

        for (destination, source) in destination.iter_mut().zip(&self.bins) {
            *destination = f32::from_bits(source.load(Ordering::Relaxed));
        }
    }

    fn publish(&self, bins: &[f32; SPECTRUM_BINS]) {
        for (destination, source) in self.bins.iter().zip(bins) {
            destination.store(source.to_bits(), Ordering::Relaxed);
        }

        self.generation.fetch_add(1, Ordering::Release);
    }
}

pub(super) struct SpectrumAnalyzer {
    output: Arc<SpectrumOutput>,
    sample_rate: f32,
    input: Box<[f32; FFT_SIZE]>,
    input_position: usize,
    window: Box<[f32; FFT_SIZE]>,
    fft: Box<[Complex; FFT_SIZE]>,
    display_bins: Box<[f32; SPECTRUM_BINS]>,
}

impl SpectrumAnalyzer {
    pub(super) fn new(output: Arc<SpectrumOutput>) -> Self {
        let mut window = Box::new([0.0; FFT_SIZE]);
        for (index, value) in window.iter_mut().enumerate() {
            *value = 0.5 - 0.5 * (TAU * index as f32 / FFT_SIZE as f32).cos();
        }

        Self {
            output,
            sample_rate: 0.0,
            input: Box::new([0.0; FFT_SIZE]),
            input_position: 0,
            window,
            fft: Box::new([Complex::default(); FFT_SIZE]),
            display_bins: Box::new([MIN_DB; SPECTRUM_BINS]),
        }
    }

    pub(super) fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.reset();
    }

    pub(super) fn reset(&mut self) {
        self.input.fill(0.0);
        self.input_position = 0;
        self.display_bins.fill(MIN_DB);
        self.output.publish(&self.display_bins);
    }

    #[inline]
    pub(super) fn push_sample(&mut self, sample: f32) {
        self.input[self.input_position] = sample;
        self.input_position += 1;

        if self.input_position == FFT_SIZE {
            self.analyze();
            self.input_position = 0;
        }
    }

    fn analyze(&mut self) {
        for index in 0..FFT_SIZE {
            self.fft[index] = Complex {
                re: self.input[index] * self.window[index],
                im: 0.0,
            };
        }

        fft_in_place(&mut self.fft);

        let nyquist = self.sample_rate * 0.5;
        for (index, display_bin) in self.display_bins.iter_mut().enumerate() {
            let normalized = index as f32 / (SPECTRUM_BINS - 1) as f32;
            let frequency = MIN_FREQUENCY * (MAX_FREQUENCY / MIN_FREQUENCY).powf(normalized);

            if frequency >= nyquist {
                *display_bin = MIN_DB;
                continue;
            }

            let fft_bin = frequency * FFT_SIZE as f32 / self.sample_rate;
            let lower_index = (fft_bin.floor() as usize).min(FFT_SIZE / 2 - 1);
            let upper_index = lower_index + 1;
            let fraction = fft_bin - lower_index as f32;
            let lower = magnitude(self.fft[lower_index]);
            let upper = magnitude(self.fft[upper_index]);
            let amplitude = (lower + (upper - lower) * fraction) * (4.0 / FFT_SIZE as f32);
            let db = 20.0 * amplitude.max(0.000_001).log10();
            let previous = *display_bin;
            let smoothing = if db > previous { 0.65 } else { 0.18 };
            *display_bin = (previous + (db - previous) * smoothing).clamp(MIN_DB, 0.0);
        }

        self.output.publish(&self.display_bins);
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Complex {
    re: f32,
    im: f32,
}

fn magnitude(value: Complex) -> f32 {
    (value.re * value.re + value.im * value.im).sqrt()
}

fn fft_in_place(values: &mut [Complex; FFT_SIZE]) {
    let mut reversed = 0;
    for index in 1..FFT_SIZE {
        let mut bit = FFT_SIZE >> 1;
        while reversed & bit != 0 {
            reversed ^= bit;
            bit >>= 1;
        }
        reversed ^= bit;

        if index < reversed {
            values.swap(index, reversed);
        }
    }

    let mut length = 2;
    while length <= FFT_SIZE {
        let angle = -TAU / length as f32;
        let phase_step = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };

        for start in (0..FFT_SIZE).step_by(length) {
            let mut phase = Complex { re: 1.0, im: 0.0 };
            for offset in 0..length / 2 {
                let even = values[start + offset];
                let odd = multiply(values[start + offset + length / 2], phase);
                values[start + offset] = Complex {
                    re: even.re + odd.re,
                    im: even.im + odd.im,
                };
                values[start + offset + length / 2] = Complex {
                    re: even.re - odd.re,
                    im: even.im - odd.im,
                };
                phase = multiply(phase, phase_step);
            }
        }

        length <<= 1;
    }
}

fn multiply(left: Complex, right: Complex) -> Complex {
    Complex {
        re: left.re * right.re - left.im * right.im,
        im: left.re * right.im + left.im * right.re,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_places_a_sine_wave_in_the_expected_bin() {
        const EXPECTED_BIN: usize = 64;
        let mut values = [Complex::default(); FFT_SIZE];
        for (index, value) in values.iter_mut().enumerate() {
            value.re = (TAU * EXPECTED_BIN as f32 * index as f32 / FFT_SIZE as f32).sin();
        }

        fft_in_place(&mut values);

        let mut peak_bin = 1;
        let mut peak_magnitude = 0.0;
        for (index, value) in values.iter().enumerate().take(FFT_SIZE / 2).skip(1) {
            let current_magnitude = magnitude(*value);
            if current_magnitude > peak_magnitude {
                peak_bin = index;
                peak_magnitude = current_magnitude;
            }
        }

        assert_eq!(peak_bin, EXPECTED_BIN);
        assert!((peak_magnitude - FFT_SIZE as f32 / 2.0).abs() < 1.0);
    }
}
