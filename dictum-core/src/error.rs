use thiserror::Error;

/// All errors produced by dictum-core.
#[derive(Debug, Error)]
pub enum DictumError {
    #[error("audio device error: {0}")]
    AudioDevice(String),

    #[error("audio stream error: {0}")]
    AudioStream(String),

    #[error("no default input device found")]
    NoDefaultInputDevice,

    #[error("ring buffer is full — pipeline cannot keep up")]
    RingBufferFull,

    #[error("inference error: {0}")]
    Inference(String),

    #[error("engine is already running")]
    AlreadyRunning,

    #[error("engine is not running")]
    NotRunning,

    #[error("ONNX session error: {0}")]
    OnnxSession(String),

    #[error("model file not found: {path}")]
    ModelNotFound { path: std::path::PathBuf },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, DictumError>;
