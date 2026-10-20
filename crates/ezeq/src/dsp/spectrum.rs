use rtrb::{Consumer, Producer, RingBuffer};
use std::f32::consts::TAU;
use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering},
    Arc, Mutex,
};

pub(crate) const SPECTRUM_BINS: usize = 256;
pub(crate) const MIN_DB: f32 = -90.0;
const FFT_SIZE: usize = 2048;
const SPECTRUM_BLOCK_SIZE: usize = 256;
const SPECTRUM_QUEUE_BLOCKS: usize = 16;
const MIN_FREQUENCY: f32 = 20.0;
const MAX_FREQUENCY: f32 = 20_000.0;

type SpectrumBlock = [f32; SPECTRUM_BLOCK_SIZE];

/// Creates the fixed-capacity, single-producer/single-consumer analyzer path.
///
/// The producer belongs exclusively to the audio thread. The consumer is
/// drained by the editor, where the FFT and display smoothing are performed.
pub(crate) fn spectrum_channel() -> (SpectrumInput, SpectrumOutput) {
    let (producer, consumer) = RingBuffer::new(SPECTRUM_QUEUE_BLOCKS);
    let sample_rate_bits = Arc::new(AtomicU32::new(48_000.0_f32.to_bits()));
    let reset_generation = Arc::new(AtomicU64::new(0));

    (
        SpectrumInput {
            producer,
            block: [0.0; SPECTRUM_BLOCK_SIZE],
            block_position: 0,
            sample_rate_bits: sample_rate_bits.clone(),
            reset_generation: reset_generation.clone(),
        },
        SpectrumOutput {
            reader: Mutex::new(SpectrumReader {
                consumer,
                analyzer: SpectrumAnalyzer::new(),
                reset_generation: 0,
            }),
            sample_rate_bits,
            reset_generation,
        },
    )
}

/// Real-time-safe input half of the spectrum channel.
pub(crate) struct SpectrumInput {
    producer: Producer<SpectrumBlock>,
    block: SpectrumBlock,
    block_position: usize,
    sample_rate_bits: Arc<AtomicU32>,
    reset_generation: Arc<AtomicU64>,
}

impl SpectrumInput {
    pub(super) fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate_bits
            .store(sample_rate.to_bits(), Ordering::Release);
        self.reset();
    }

    pub(super) fn reset(&mut self) {
        self.block.fill(0.0);
        self.block_position = 0;
        self.reset_generation.fetch_add(1, Ordering::Release);
    }

    #[inline]
    pub(super) fn push_sample(&mut self, sample: f32) {
        self.block[self.block_position] = sample;
        self.block_position += 1;

        if self.block_position == SPECTRUM_BLOCK_SIZE {
            // A stalled or closed editor must never block the audio callback.
            // Dropping the newest block also bounds analyzer latency.
            let _ = self.producer.push(self.block);
            self.block_position = 0;
        }
    }
}

/// Editor-side output half of the bounded spectrum channel.
pub(crate) struct SpectrumOutput {
    reader: Mutex<SpectrumReader>,
    sample_rate_bits: Arc<AtomicU32>,
    reset_generation: Arc<AtomicU64>,
}

struct SpectrumReader {
    consumer: Consumer<SpectrumBlock>,
    analyzer: SpectrumAnalyzer,
    reset_generation: u64,
}

impl SpectrumOutput {
    pub(crate) fn read(&self, destination: &mut [f32; SPECTRUM_BINS]) {
        let mut reader = self
            .reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let reset_generation = self.reset_generation.load(Ordering::Acquire);
        if reader.reset_generation != reset_generation {
            reader.reset_generation = reset_generation;
            reader.analyzer.prepare(self.sample_rate());
        }

        while let Ok(block) = reader.consumer.pop() {
            for sample in block {
                reader.analyzer.push_sample(sample);
            }
        }
        destination.copy_from_slice(reader.analyzer.display_bins.as_slice());
    }

    pub(crate) fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate_bits.load(Ordering::Acquire))
    }
}

struct SpectrumAnalyzer {
    sample_rate: f32,
    input: Box<[f32; FFT_SIZE]>,
    input_position: usize,
    window: Box<[f32; FFT_SIZE]>,
    fft: Box<[Complex; FFT_SIZE]>,
    display_bins: Box<[f32; SPECTRUM_BINS]>,
}

impl SpectrumAnalyzer {
    fn new() -> Self {
        let mut window = Box::new([0.0; FFT_SIZE]);
        for (index, value) in window.iter_mut().enumerate() {
            *value = 0.5 - 0.5 * (TAU * index as f32 / FFT_SIZE as f32).cos();
        }

        Self {
            sample_rate: 48_000.0,
            input: Box::new([0.0; FFT_SIZE]),
            input_position: 0,
            window,
            fft: Box::new([Complex::default(); FFT_SIZE]),
            display_bins: Box::new([MIN_DB; SPECTRUM_BINS]),
        }
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.reset();
    }

    fn reset(&mut self) {
        self.input.fill(0.0);
        self.input_position = 0;
        self.display_bins.fill(MIN_DB);
    }

    #[inline]
    fn push_sample(&mut self, sample: f32) {
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
    fn bounded_channel_moves_fft_work_to_the_consumer() {
        const EXPECTED_BIN: usize = 64;
        let (mut input, output) = spectrum_channel();
        input.prepare(48_000.0);

        for index in 0..FFT_SIZE {
            input.push_sample((TAU * EXPECTED_BIN as f32 * index as f32 / FFT_SIZE as f32).sin());
        }

        let mut levels = [MIN_DB; SPECTRUM_BINS];
        output.read(&mut levels);

        assert!(levels.into_iter().any(|level| level > -40.0));
    }

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
