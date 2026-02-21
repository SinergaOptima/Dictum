//! `DictumEngine` — top-level lifecycle controller.
//!
//! ## Lifecycle
//!
//! ```text
//! DictumEngine::new()
//!     └─► warm_up()          → model loaded, status = WarmingUp → Idle
//!         └─► start()        → audio open, pipeline spawned, status = Listening
//!             └─► stop()     → running=false, stream dropped, status = Stopped
//! ```
//!
//! `start()`/`stop()` are idempotent: calling them in the wrong state returns
//! an error rather than panicking.
//!
//! ## Threading
//!
//! `cpal::Stream` is `!Send` on Windows/macOS (COM / CoreAudio thread affinity).
//! `AudioCapture` is therefore created *inside* the `spawn_blocking` closure so
//! it never crosses a thread boundary. A sync oneshot channel propagates any
//! open-device errors back to the `start()` caller.

pub mod pipeline;

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use parking_lot::Mutex;
use tokio::sync::broadcast;
use tracing::info;

use crate::{
    audio::AudioCapture,
    buffering::create_audio_ring,
    error::{DictumError, Result},
    inference::ModelHandle,
    ipc::events::{AudioActivityEvent, EngineStatus, EngineStatusEvent, TranscriptEvent},
    vad::{energy::EnergyVad, VoiceActivityDetector},
};

#[cfg(feature = "onnx")]
use crate::vad::SileroVad;

/// Broadcast channel capacity: 256 transcript events buffered for slow consumers.
const BROADCAST_CAP: usize = 256;

/// Configuration for `DictumEngine`.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Target sample rate for inference (Hz). Audio captured at other rates
    /// will be resampled (Phase 1). Default: 16000.
    pub target_sample_rate: u32,
    /// VAD RMS threshold. Default: 0.02.
    pub vad_threshold: f32,
    /// VAD hangover in frames. Default: 8.
    pub vad_hangover_frames: u32,
    /// Silero VAD speech probability threshold in [0, 1].
    /// Default: 0.20.
    #[cfg(feature = "onnx")]
    pub silero_vad_threshold: f32,
    /// Minimum speech duration (samples at `target_sample_rate`) before
    /// inference is triggered. Default: 8000 (0.5 s).
    pub min_speech_samples: usize,
    /// Maximum accumulated speech (samples) before a forced inference.
    /// Default: 480000 (30 s at 16 kHz).
    pub max_speech_samples: usize,
    /// Whether to emit partial inference updates during active speech.
    /// Partial decoding improves live preview but can increase CPU/GPU load.
    pub enable_partial_inference: bool,
    /// Override path for the Silero VAD ONNX model.
    /// `None` falls back to the platform default models directory.
    #[cfg(feature = "onnx")]
    pub silero_vad_path: Option<std::path::PathBuf>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            target_sample_rate: 16_000,
            vad_threshold: 0.01, // Lowered from 0.02 for quieter microphones
            vad_hangover_frames: 8,
            #[cfg(feature = "onnx")]
            silero_vad_threshold: 0.20,
            min_speech_samples: 4_000, // Lowered from 8000 (0.25s instead of 0.5s)
            max_speech_samples: 480_000,
            enable_partial_inference: true,
            #[cfg(feature = "onnx")]
            silero_vad_path: None,
        }
    }
}

/// The top-level engine handle.
///
/// `DictumEngine` is `Send + Sync` — all fields use interior mutability.
/// Wrap in `Arc<DictumEngine>` to share between the Tauri app state and
/// event-forwarding async tasks.
pub struct DictumEngine {
    config: EngineConfig,
    model: ModelHandle,
    /// `true` while capture + pipeline are active.
    running: Arc<AtomicBool>,
    /// Canonical status (written atomically via Mutex, read from commands).
    status: Arc<Mutex<EngineStatus>>,
    /// Broadcast sender for transcript events.
    transcript_tx: broadcast::Sender<TranscriptEvent>,
    /// Broadcast sender for status events.
    status_tx: broadcast::Sender<EngineStatusEvent>,
    /// Broadcast sender for live VAD / level activity events.
    activity_tx: broadcast::Sender<AudioActivityEvent>,
    /// Monotonically increasing event sequence counter.
    seq: Arc<AtomicU64>,
    /// Shared pipeline diagnostics counters.
    diagnostics: Arc<pipeline::PipelineDiagnostics>,
}

impl DictumEngine {
    /// Create a new engine. Does not start capturing — call `warm_up()` then `start()`.
    pub fn new(config: EngineConfig, model: ModelHandle) -> Self {
        let (transcript_tx, _) = broadcast::channel(BROADCAST_CAP);
        let (status_tx, _) = broadcast::channel(BROADCAST_CAP);
        let (activity_tx, _) = broadcast::channel(BROADCAST_CAP);
        let diagnostics = Arc::new(pipeline::PipelineDiagnostics::default());

        Self {
            config,
            model,
            running: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(EngineStatus::Idle)),
            transcript_tx,
            status_tx,
            activity_tx,
            seq: Arc::new(AtomicU64::new(0)),
            diagnostics,
        }
    }

    /// Warm up the speech model (load weights, run dummy inference).
    ///
    /// Call once at application startup, before `start()`.
    pub fn warm_up(&self) -> Result<()> {
        self.set_status(EngineStatus::WarmingUp, None);
        info!("warming up speech model");
        self.model.0.lock().warm_up()?;
        self.set_status(EngineStatus::Idle, None);
        info!("speech model ready");
        Ok(())
    }

    /// Start audio capture and the pipeline.
    ///
    /// Blocks until the audio device is confirmed open (or fails), then returns.
    /// The pipeline continues running in a background blocking thread.
    ///
    /// # Errors
    /// - `DictumError::AlreadyRunning` if already started.
    /// - `DictumError::NoDefaultInputDevice` / `DictumError::AudioStream` on device error.
    pub fn start(&self) -> Result<()> {
        self.start_with_device(None)
    }

    /// Start the engine using a preferred input device name.
    ///
    /// If `preferred_input_device` is `None`, default input selection is used.
    pub fn start_with_device(&self, preferred_input_device: Option<String>) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Err(DictumError::AlreadyRunning);
        }

        self.diagnostics.reset();
        self.running.store(true, Ordering::SeqCst);
        self.set_status(EngineStatus::Listening, None);

        let (producer, consumer) = create_audio_ring();

        // Clone all Arc-wrapped state before moving into the closure.
        let config = self.config.clone();
        let model = self.model.clone();
        let running = Arc::clone(&self.running);
        let transcript_tx = self.transcript_tx.clone();
        let status_tx = self.status_tx.clone();
        let activity_tx = self.activity_tx.clone();
        let status = Arc::clone(&self.status);
        let seq = Arc::clone(&self.seq);
        let diagnostics = Arc::clone(&self.diagnostics);
        let preferred_input_device = preferred_input_device.clone();

        // Sync oneshot: pipeline thread signals open success/failure to start().
        // Carries the actual capture sample rate on success.
        let (open_tx, open_rx) = std::sync::mpsc::channel::<Result<u32>>();

        tokio::task::spawn_blocking(move || {
            // ── Open audio device (must happen on THIS thread — cpal::Stream is !Send) ──
            let capture = match AudioCapture::open_with_preference(
                producer,
                Arc::clone(&running),
                preferred_input_device.as_deref(),
            ) {
                Ok(c) => {
                    let _ = open_tx.send(Ok(c.sample_rate));
                    c
                }
                Err(e) => {
                    let _ = open_tx.send(Err(e));
                    running.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let capture_sample_rate = capture.sample_rate;

            // ── Select VAD ────────────────────────────────────────────────────────────
            #[cfg(feature = "onnx")]
            let vad: Box<dyn VoiceActivityDetector> = {
                let path = config
                    .silero_vad_path
                    .clone()
                    .unwrap_or_else(SileroVad::default_model_path);
                let silero_threshold = config.silero_vad_threshold.clamp(0.03, 0.95);
                match SileroVad::new(&path, silero_threshold) {
                    Ok(v) => {
                        info!(
                            "using SileroVad from {:?} with threshold={}",
                            path, silero_threshold
                        );
                        Box::new(v)
                    }
                    Err(e) => {
                        tracing::warn!("SileroVad load failed ({e}), falling back to EnergyVad");
                        Box::new(EnergyVad::new(
                            config.vad_threshold,
                            config.vad_hangover_frames,
                        ))
                    }
                }
            };

            #[cfg(not(feature = "onnx"))]
            let vad: Box<dyn VoiceActivityDetector> = Box::new(EnergyVad::new(
                config.vad_threshold,
                config.vad_hangover_frames,
            ));

            // ── Run pipeline ──────────────────────────────────────────────────────────
            pipeline::run(pipeline::PipelineContext {
                config,
                model,
                vad,
                consumer,
                running,
                transcript_tx,
                status_tx,
                activity_tx,
                status,
                seq,
                capture_sample_rate,
                diagnostics,
            });

            // Stream drops here, releasing the audio device on this thread.
            drop(capture);
        });

        // Block start() until device open is confirmed (receives actual sample rate).
        match open_rx.recv() {
            Ok(Ok(_rate)) => {
                info!("engine started — listening");
                Ok(())
            }
            Ok(Err(e)) => {
                self.running.store(false, Ordering::SeqCst);
                self.set_status(EngineStatus::Error, Some(e.to_string()));
                Err(e)
            }
            Err(_) => {
                // Channel closed before a message was sent — spawn_blocking panicked?
                self.running.store(false, Ordering::SeqCst);
                self.set_status(EngineStatus::Error, Some("pipeline failed to start".into()));
                Err(DictumError::Other(anyhow::anyhow!(
                    "pipeline task died unexpectedly"
                )))
            }
        }
    }

    /// Stop audio capture and the pipeline.
    ///
    /// # Errors
    /// - `DictumError::NotRunning` if not currently running.
    pub fn stop(&self) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(DictumError::NotRunning);
        }

        self.running.store(false, Ordering::SeqCst);
        self.set_status(EngineStatus::Stopped, None);
        info!("engine stop requested");
        Ok(())
    }

    /// Current engine status (snapshot).
    pub fn status(&self) -> EngineStatus {
        *self.status.lock()
    }

    /// Subscribe to live transcript events.
    pub fn subscribe_transcripts(&self) -> broadcast::Receiver<TranscriptEvent> {
        self.transcript_tx.subscribe()
    }

    /// Subscribe to live status change events.
    pub fn subscribe_status(&self) -> broadcast::Receiver<EngineStatusEvent> {
        self.status_tx.subscribe()
    }

    /// Subscribe to live voice activity events (RMS + speech classification).
    pub fn subscribe_activity(&self) -> broadcast::Receiver<AudioActivityEvent> {
        self.activity_tx.subscribe()
    }

    /// Snapshot of pipeline counters for observability.
    pub fn pipeline_diagnostics_snapshot(&self) -> pipeline::DiagnosticsSnapshot {
        self.diagnostics.snapshot()
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn set_status(&self, new_status: EngineStatus, detail: Option<String>) {
        *self.status.lock() = new_status;
        let _ = self.status_tx.send(EngineStatusEvent {
            status: new_status,
            detail,
        });
    }
}
