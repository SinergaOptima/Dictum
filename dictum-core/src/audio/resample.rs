//! Audio sample-rate conversion using a rubato `FastFixedIn` resampler.
//!
//! ## Design
//!
//! `cpal` captures audio at the device's native rate (commonly 48 kHz on
//! Windows). Whisper requires 16 kHz mono f32. `RateConverter` bridges that
//! gap on the non-RT pipeline thread, where allocation is allowed.
//!
//! When capture rate == target rate, `RateConverter` is a zero-copy
//! passthrough — no rubato session is created at all.
//!
//! ## Usage
//!
//! ```ignore
//! let mut rc = RateConverter::new(48_000, 16_000, 960)?;
//! let out = rc.process(&raw_samples); // Vec<f32> at 16 kHz
//! ```

use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use tracing::error;

use crate::error::{DictumError, Result};

/// Converts f32 mono audio from one fixed sample rate to another.
pub struct RateConverter {
    /// `None` when capture rate == target rate (passthrough mode).
    resampler: Option<FastFixedIn<f32>>,
    /// Accumulation buffer — holds partial input chunks between calls.
    input_buf: Vec<f32>,
    /// How many input samples rubato expects per process call.
    chunk_size: usize,
    /// Pre-allocated output buffer: `[1][output_frames_max]`.
    output_buf: Vec<Vec<f32>>,
}

impl RateConverter {
    /// Create a new converter.
    ///
    /// # Parameters
    /// - `capture_rate`: Sample rate of the incoming audio (Hz).
    /// - `target_rate`: Sample rate expected by the model (Hz).
    /// - `chunk_size`: Input frame count per rubato call (e.g. `960`).
    ///
    /// # Errors
    /// Returns `DictumError::AudioDevice` if rubato fails to initialise.
    pub fn new(capture_rate: u32, target_rate: u32, chunk_size: usize) -> Result<Self> {
        if capture_rate == target_rate {
            return Ok(Self {
                resampler: None,
                input_buf: Vec::new(),
                chunk_size,
                output_buf: Vec::new(),
            });
        }

        let ratio = target_rate as f64 / capture_rate as f64;

        let resampler = FastFixedIn::<f32>::new(
            ratio,
            1.0, // fixed ratio — no dynamic adjustment
            PolynomialDegree::Cubic,
            chunk_size,
            1, // mono
        )
        .map_err(|e| DictumError::AudioDevice(format!("resampler init: {e}")))?;

        let max_out = resampler.output_frames_max();
        let output_buf = vec![vec![0f32; max_out]; 1];

        tracing::info!(
            capture_rate,
            target_rate,
            chunk_size,
            max_out,
            "resampling enabled from={} to={}",
            capture_rate,
            target_rate
        );

        Ok(Self {
            resampler: Some(resampler),
            input_buf: Vec::new(),
            chunk_size,
            output_buf,
        })
    }

    /// Process incoming samples, returning resampled output (may be empty).
    ///
    /// Samples are accumulated internally until a full `chunk_size` block is
    /// available for rubato. Any remainder is kept for the next call.
    ///
    /// In passthrough mode (same rates), input is returned directly.
    pub fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        let Some(ref mut resampler) = self.resampler else {
            // Zero-copy passthrough
            return samples.to_vec();
        };

        self.input_buf.extend_from_slice(samples);

        let mut result = Vec::new();

        while self.input_buf.len() >= self.chunk_size {
            let input_slice = &self.input_buf[..self.chunk_size];

            match resampler.process_into_buffer(&[input_slice], &mut self.output_buf, None) {
                Ok((_consumed, produced)) => {
                    result.extend_from_slice(&self.output_buf[0][..produced]);
                }
                Err(e) => {
                    error!("resampler process error: {e}");
                }
            }

            self.input_buf.drain(..self.chunk_size);
        }

        result
    }

    /// Returns `true` when capture rate == target rate (no resampling occurs).
    pub fn is_passthrough(&self) -> bool {
        self.resampler.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_identity() {
        let mut rc = RateConverter::new(16_000, 16_000, 960).unwrap();
        assert!(rc.is_passthrough());
        let samples: Vec<f32> = (0..480).map(|i| i as f32 * 0.001).collect();
        let out = rc.process(&samples);
        assert_eq!(out, samples);
    }

    #[test]
    fn ratio_48k_to_16k_correct_length() {
        let mut rc = RateConverter::new(48_000, 16_000, 960).unwrap();
        assert!(!rc.is_passthrough());
        // 960 input samples at 48 kHz → ~320 at 16 kHz
        let samples = vec![0.0f32; 960];
        let out = rc.process(&samples);
        assert!(!out.is_empty(), "expected non-empty output");
        let expected = 320usize;
        assert!(
            (out.len() as isize - expected as isize).unsigned_abs() <= 10,
            "output len={} expected≈{}",
            out.len(),
            expected
        );
    }

    #[test]
    fn partial_accumulation_returns_empty() {
        let mut rc = RateConverter::new(48_000, 16_000, 960).unwrap();
        // Fewer than chunk_size samples → nothing output yet
        let samples = vec![0.0f32; 500];
        let out = rc.process(&samples);
        assert!(
            out.is_empty(),
            "expected empty output for partial chunk, got {}",
            out.len()
        );
    }

    #[test]
    fn multiple_partial_chunks_accumulate() {
        let mut rc = RateConverter::new(48_000, 16_000, 960).unwrap();
        // Two 500-sample pushes = 1000 total ≥ 960 chunk_size → should produce output
        let out1 = rc.process(&vec![0.0f32; 500]);
        assert!(out1.is_empty());
        let out2 = rc.process(&vec![0.0f32; 500]);
        assert!(!out2.is_empty(), "second push should trigger processing");
    }
}
