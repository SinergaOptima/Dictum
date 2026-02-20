//! Typed audio chunk passed from the ring buffer to the VAD and inference stages.

/// A contiguous block of mono PCM samples at a known sample rate.
///
/// Allocated once per pipeline iteration (on the non-RT pipeline thread).
#[derive(Debug, Clone)]
pub struct AudioChunk {
    /// Mono f32 samples in [-1.0, 1.0].
    pub samples: Vec<f32>,
    /// Sample rate in Hz (e.g. 16000, 44100, 48000).
    pub sample_rate: u32,
}

impl AudioChunk {
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
        }
    }

    /// Returns the duration of this chunk in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }

    /// Returns true if the chunk contains no samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}
