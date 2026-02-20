//! # dictum-core
//!
//! Reusable voice-to-text engine SDK.
//!
//! ## Architecture
//!
//! ```text
//! Microphone → AudioCapture → SPSC RingBuffer → Pipeline(spawn_blocking)
//!                                                    │
//!                                              VAD decision
//!                                                    │
//!                                           SpeechModel::transcribe
//!                                                    │
//!                                          broadcast::Sender<TranscriptEvent>
//! ```
//!
//! The audio callback is zero-alloc. All heap work happens in the pipeline thread.

#![forbid(unsafe_code)]
#![warn(clippy::all)]

pub mod audio;
pub mod buffering;
pub mod engine;
pub mod error;
pub mod inference;
pub mod ipc;
pub mod vad;

// Convenience re-exports for downstream crates
pub use engine::{DictumEngine, EngineConfig};
pub use error::DictumError;
pub use inference::{ModelHandle, SpeechModel};
pub use ipc::events::{
    AudioActivityEvent, EngineStatus, EngineStatusEvent, TranscriptEvent, TranscriptSegment,
};

#[cfg(feature = "onnx")]
pub use inference::{OnnxModel, OnnxModelConfig};

#[cfg(feature = "onnx")]
pub use vad::SileroVad;
