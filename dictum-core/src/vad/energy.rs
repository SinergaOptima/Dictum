//! Energy-based VAD using RMS threshold + hangover counter.
//!
//! ## Algorithm
//!
//! 1. Compute RMS of the incoming chunk.
//! 2. If RMS ≥ `threshold` → emit `Speech`, reset hangover counter.
//! 3. If RMS < `threshold` and hangover counter > 0 → emit `Speech`,
//!    decrement counter (prevents clipping syllable endings).
//! 4. Otherwise → emit `Silence`.

use super::{VadDecision, VoiceActivityDetector};
use crate::buffering::chunk::AudioChunk;

/// A simple energy-based voice activity detector.
#[derive(Debug, Clone)]
pub struct EnergyVad {
    /// RMS amplitude threshold. Frames above this are considered speech.
    /// Typical range: 0.01–0.05 for a quiet microphone.
    threshold: f32,
    /// How many consecutive below-threshold frames to still emit `Speech`
    /// after real speech ends (prevents clipping word endings).
    hangover_frames: u32,
    /// Current hangover countdown.
    hangover_counter: u32,
}

impl EnergyVad {
    /// Create a new `EnergyVad`.
    ///
    /// # Parameters
    /// - `threshold`: RMS level above which a frame is considered speech.
    ///   Default: `0.02`.
    /// - `hangover_frames`: Number of silent frames to extend speech detection.
    ///   Default: `8` (≈ 160 ms at a 20 ms frame stride).
    pub fn new(threshold: f32, hangover_frames: u32) -> Self {
        Self {
            threshold,
            hangover_frames,
            hangover_counter: 0,
        }
    }

    /// Compute the root-mean-square of a sample slice.
    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }
}

impl Default for EnergyVad {
    fn default() -> Self {
        Self::new(0.02, 8)
    }
}

impl VoiceActivityDetector for EnergyVad {
    fn classify(&mut self, chunk: &AudioChunk) -> VadDecision {
        let rms = Self::rms(&chunk.samples);

        if rms >= self.threshold {
            // Active speech detected — reset hangover
            self.hangover_counter = self.hangover_frames;
            VadDecision::Speech
        } else if self.hangover_counter > 0 {
            // Within hangover window — still report speech
            self.hangover_counter -= 1;
            VadDecision::Speech
        } else {
            VadDecision::Silence
        }
    }

    fn reset(&mut self) {
        self.hangover_counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffering::chunk::AudioChunk;

    fn silent_chunk(len: usize) -> AudioChunk {
        AudioChunk::new(vec![0.0f32; len], 16000)
    }

    fn loud_chunk(amplitude: f32, len: usize) -> AudioChunk {
        AudioChunk::new(vec![amplitude; len], 16000)
    }

    #[test]
    fn silence_below_threshold() {
        let mut vad = EnergyVad::new(0.02, 0);
        let chunk = silent_chunk(160);
        assert_eq!(vad.classify(&chunk), VadDecision::Silence);
    }

    #[test]
    fn speech_above_threshold() {
        let mut vad = EnergyVad::new(0.02, 0);
        let chunk = loud_chunk(0.5, 160);
        assert_eq!(vad.classify(&chunk), VadDecision::Speech);
    }

    #[test]
    fn hangover_extends_speech() {
        let mut vad = EnergyVad::new(0.02, 3);

        // One loud frame triggers speech
        assert_eq!(vad.classify(&loud_chunk(0.5, 160)), VadDecision::Speech);

        // Next 3 silent frames should still be Speech (hangover)
        assert_eq!(vad.classify(&silent_chunk(160)), VadDecision::Speech);
        assert_eq!(vad.classify(&silent_chunk(160)), VadDecision::Speech);
        assert_eq!(vad.classify(&silent_chunk(160)), VadDecision::Speech);

        // 4th silent frame: hangover exhausted → Silence
        assert_eq!(vad.classify(&silent_chunk(160)), VadDecision::Silence);
    }

    #[test]
    fn reset_clears_hangover() {
        let mut vad = EnergyVad::new(0.02, 5);
        vad.classify(&loud_chunk(0.5, 160));
        vad.reset();
        // After reset, next silent frame should be Silence immediately
        assert_eq!(vad.classify(&silent_chunk(160)), VadDecision::Silence);
    }

    #[test]
    fn empty_chunk_is_silence() {
        let mut vad = EnergyVad::default();
        let chunk = AudioChunk::new(vec![], 16000);
        assert_eq!(vad.classify(&chunk), VadDecision::Silence);
    }

    #[test]
    fn rms_of_unit_sine_approximation() {
        // A square wave at ±0.5 should have RMS = 0.5
        let samples: Vec<f32> = (0..256)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let rms = EnergyVad::rms(&samples);
        // RMS of ±0.5 square wave = 0.5
        assert!((rms - 0.5).abs() < 1e-5, "rms={rms}");
    }
}
