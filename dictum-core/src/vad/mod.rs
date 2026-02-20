//! Voice Activity Detection (VAD) abstraction.
//!
//! The `VoiceActivityDetector` trait is the primary extensibility point:
//! swap in `EnergyVad` (default), SileroVad (P1-08), or any future neural VAD
//! without touching the pipeline.

pub mod energy;

#[cfg(feature = "onnx")]
pub mod silero;

#[cfg(feature = "onnx")]
pub use silero::SileroVad;

use crate::buffering::chunk::AudioChunk;

/// Whether a given audio frame contains speech or silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadDecision {
    /// The frame contains speech energy above threshold.
    Speech,
    /// The frame is silent (or below threshold, including hangover period).
    Silence,
}

impl VadDecision {
    pub fn is_speech(self) -> bool {
        self == VadDecision::Speech
    }
}

/// Trait for all VAD implementations.
///
/// Implementors may be stateful (hangover counters, RNN hidden states, etc.).
pub trait VoiceActivityDetector: Send + 'static {
    /// Analyse a chunk and return a speech/silence decision.
    ///
    /// The chunk's `sample_rate` should match whatever rate this detector
    /// was configured for. Resampling is the caller's responsibility.
    fn classify(&mut self, chunk: &AudioChunk) -> VadDecision;

    /// Reset any internal state (e.g. hangover counters, hidden states).
    fn reset(&mut self);
}
