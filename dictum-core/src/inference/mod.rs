//! Speech model abstraction.
//!
//! The `SpeechModel` trait decouples the pipeline from any specific backend
//! (stub echo, ONNX Whisper, GGUF Whisper.cpp, etc.).
//!
//! `&mut self` on `transcribe` intentionally expresses that decoders are
//! stateful — beam search caches, RNN hidden states, etc. All mutation is
//! therefore serialised through `ModelHandle`'s `parking_lot::Mutex`.

pub mod stub;

#[cfg(feature = "onnx")]
pub mod onnx;

#[cfg(feature = "onnx")]
pub use onnx::{OnnxModel, OnnxModelConfig};

use std::sync::Arc;

use parking_lot::Mutex;

use crate::buffering::chunk::AudioChunk;
use crate::error::Result;
use crate::ipc::events::TranscriptSegment;

/// Contract for speech recognition backends.
pub trait SpeechModel: Send + 'static {
    /// One-time warm-up: load weights, pre-allocate KV caches, run a dummy
    /// inference to populate CPU caches. Called once at engine startup.
    ///
    /// # Errors
    /// Returns an error if model files are missing or corrupt.
    fn warm_up(&mut self) -> Result<()>;

    /// Transcribe a mono f32 audio chunk.
    ///
    /// # Parameters
    /// - `chunk`: Audio data. Implementations may resample internally if needed.
    /// - `partial`: If `true`, the caller requests a partial (streaming) result.
    ///   The model may return fewer words or a lower-confidence hypothesis.
    ///
    /// # Returns
    /// A list of `TranscriptSegment`s. May be empty if no speech was detected.
    fn transcribe(&mut self, chunk: &AudioChunk, partial: bool) -> Result<Vec<TranscriptSegment>>;

    /// Reset all internal decoder state (e.g. between utterances).
    fn reset(&mut self);
}

/// Thread-safe reference-counted handle to any `SpeechModel` implementor.
///
/// Uses `parking_lot::Mutex` for:
/// - Non-poisoning on panic (unlike `std::sync::Mutex`)
/// - ~25 % faster uncontended lock on x86-64 Windows
#[derive(Clone)]
pub struct ModelHandle(pub Arc<Mutex<dyn SpeechModel>>);

impl ModelHandle {
    /// Wrap any `SpeechModel` in a `ModelHandle`.
    pub fn new<M: SpeechModel>(model: M) -> Self {
        Self(Arc::new(Mutex::new(model)))
    }
}

impl std::fmt::Debug for ModelHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelHandle").finish_non_exhaustive()
    }
}
