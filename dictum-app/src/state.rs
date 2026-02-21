//! Tauri application state.
//!
//! `AppState` is managed via `app.manage(...)` and injected into command handlers
//! by Tauri's `State<'_, AppState>` extractor.

use dictum_core::DictumEngine;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use crate::settings::{AppSettings, LearnedCorrection};
use crate::storage::LocalStore;
use crate::transform::TextTransform;

/// Shared application state — available in every `#[tauri::command]`.
pub struct AppState {
    /// The core engine. Wrapped in `Arc` so it can be cloned into event-forwarding
    /// tasks started after setup.
    pub engine: Arc<DictumEngine>,
    /// User-selected microphone name to use when starting capture.
    pub preferred_input_device: Arc<Mutex<Option<String>>>,
    /// Count of text injection attempts.
    pub inject_calls: Arc<AtomicUsize>,
    /// Count of successful text injections.
    pub inject_success: Arc<AtomicUsize>,
    /// Count of final transcript segments observed.
    pub final_segments_seen: Arc<AtomicUsize>,
    /// Count of emitted fallback placeholders that were typed.
    pub fallback_stub_typed: Arc<AtomicUsize>,
    /// Guard to prevent overlapping shortcut-triggered toggle operations.
    pub shortcut_toggle_inflight: Arc<AtomicBool>,
    /// Count of shortcut toggles accepted for execution.
    pub shortcut_toggle_executed: Arc<AtomicUsize>,
    /// Count of shortcut toggles dropped due to overlap/race protection.
    pub shortcut_toggle_dropped: Arc<AtomicUsize>,
    /// Persisted app settings cache.
    pub settings: Arc<Mutex<AppSettings>>,
    /// Learned transcript correction rules used for live cleanup.
    pub learned_corrections: Arc<RwLock<Vec<LearnedCorrection>>>,
    /// Absolute path to `settings.json`.
    pub settings_path: PathBuf,
    /// Local encrypted SQLite storage.
    pub store: Arc<LocalStore>,
    /// In-memory dictionary/snippet transform engine.
    pub transformer: Arc<TextTransform>,
    /// Rolling stage latency metrics.
    pub perf_metrics: Arc<Mutex<PerfMetrics>>,
}

impl AppState {
    pub fn diagnostics_snapshot(&self) -> AppDiagnostics {
        let pipeline = self.engine.pipeline_diagnostics_snapshot();
        AppDiagnostics {
            inject_calls: self.inject_calls.load(Ordering::Relaxed),
            inject_success: self.inject_success.load(Ordering::Relaxed),
            final_segments_seen: self.final_segments_seen.load(Ordering::Relaxed),
            fallback_stub_typed: self.fallback_stub_typed.load(Ordering::Relaxed),
            shortcut_toggle_executed: self.shortcut_toggle_executed.load(Ordering::Relaxed),
            shortcut_toggle_dropped: self.shortcut_toggle_dropped.load(Ordering::Relaxed),
            pipeline_frames_in: pipeline.frames_in,
            pipeline_frames_resampled: pipeline.frames_resampled,
            pipeline_vad_windows: pipeline.vad_windows,
            pipeline_vad_speech: pipeline.vad_speech,
            pipeline_inference_calls: pipeline.inference_calls,
            pipeline_inference_errors: pipeline.inference_errors,
            pipeline_segments_emitted: pipeline.segments_emitted,
            pipeline_fallback_emitted: pipeline.fallback_emitted,
        }
    }

    pub fn perf_snapshot(&self) -> PerfSnapshot {
        let diag = self.diagnostics_snapshot();
        let metrics = self.perf_metrics.lock().snapshot();
        PerfSnapshot {
            diagnostics: diag,
            transform_ms: metrics.transform_ms,
            inject_ms: metrics.inject_ms,
            persist_ms: metrics.persist_ms,
            finalize_ms: metrics.finalize_ms,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AppDiagnostics {
    pub inject_calls: usize,
    pub inject_success: usize,
    pub final_segments_seen: usize,
    pub fallback_stub_typed: usize,
    pub shortcut_toggle_executed: usize,
    pub shortcut_toggle_dropped: usize,
    pub pipeline_frames_in: usize,
    pub pipeline_frames_resampled: usize,
    pub pipeline_vad_windows: usize,
    pub pipeline_vad_speech: usize,
    pub pipeline_inference_calls: usize,
    pub pipeline_inference_errors: usize,
    pub pipeline_segments_emitted: usize,
    pub pipeline_fallback_emitted: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfSnapshot {
    pub diagnostics: AppDiagnostics,
    pub transform_ms: PerfStageSnapshot,
    pub inject_ms: PerfStageSnapshot,
    pub persist_ms: PerfStageSnapshot,
    pub finalize_ms: PerfStageSnapshot,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfStageSnapshot {
    pub count: usize,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Default)]
pub struct PerfMetrics {
    transform_ms: StageWindow,
    inject_ms: StageWindow,
    persist_ms: StageWindow,
    finalize_ms: StageWindow,
}

impl PerfMetrics {
    pub fn record_transform(&mut self, elapsed_ms: f64) {
        self.transform_ms.record(elapsed_ms);
    }

    pub fn record_inject(&mut self, elapsed_ms: f64) {
        self.inject_ms.record(elapsed_ms);
    }

    pub fn record_persist(&mut self, elapsed_ms: f64) {
        self.persist_ms.record(elapsed_ms);
    }

    pub fn record_finalize(&mut self, elapsed_ms: f64) {
        self.finalize_ms.record(elapsed_ms);
    }

    pub fn snapshot(&self) -> PerfMetricsSnapshot {
        PerfMetricsSnapshot {
            transform_ms: self.transform_ms.snapshot(),
            inject_ms: self.inject_ms.snapshot(),
            persist_ms: self.persist_ms.snapshot(),
            finalize_ms: self.finalize_ms.snapshot(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PerfMetricsSnapshot {
    pub transform_ms: PerfStageSnapshot,
    pub inject_ms: PerfStageSnapshot,
    pub persist_ms: PerfStageSnapshot,
    pub finalize_ms: PerfStageSnapshot,
}

#[derive(Debug)]
struct StageWindow {
    samples: VecDeque<f64>,
    cap: usize,
    count: usize,
    sum_ms: f64,
    max_ms: f64,
}

impl Default for StageWindow {
    fn default() -> Self {
        Self {
            samples: VecDeque::with_capacity(512),
            cap: 512,
            count: 0,
            sum_ms: 0.0,
            max_ms: 0.0,
        }
    }
}

impl StageWindow {
    fn record(&mut self, elapsed_ms: f64) {
        let v = if elapsed_ms.is_finite() {
            elapsed_ms.max(0.0)
        } else {
            0.0
        };
        if self.samples.len() == self.cap {
            let _ = self.samples.pop_front();
        }
        self.samples.push_back(v);
        self.count = self.count.saturating_add(1);
        self.sum_ms += v;
        if v > self.max_ms {
            self.max_ms = v;
        }
    }

    fn snapshot(&self) -> PerfStageSnapshot {
        if self.samples.is_empty() {
            return PerfStageSnapshot {
                count: 0,
                mean_ms: 0.0,
                p50_ms: 0.0,
                p95_ms: 0.0,
                p99_ms: 0.0,
                max_ms: 0.0,
            };
        }
        let mut sorted: Vec<f64> = self.samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.total_cmp(b));

        let percentile = |p: f64| -> f64 {
            let n = sorted.len();
            if n == 1 {
                return sorted[0];
            }
            let idx = ((n - 1) as f64 * p).round() as usize;
            sorted[idx.min(n - 1)]
        };

        PerfStageSnapshot {
            count: self.count,
            mean_ms: if self.count == 0 {
                0.0
            } else {
                self.sum_ms / self.count as f64
            },
            p50_ms: percentile(0.50),
            p95_ms: percentile(0.95),
            p99_ms: percentile(0.99),
            max_ms: self.max_ms,
        }
    }
}

impl Serialize for AppDiagnostics {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Repr {
            inject_calls: usize,
            inject_success: usize,
            final_segments_seen: usize,
            fallback_stub_typed: usize,
            shortcut_toggle_executed: usize,
            shortcut_toggle_dropped: usize,
            pipeline_frames_in: usize,
            pipeline_frames_resampled: usize,
            pipeline_vad_windows: usize,
            pipeline_vad_speech: usize,
            pipeline_inference_calls: usize,
            pipeline_inference_errors: usize,
            pipeline_segments_emitted: usize,
            pipeline_fallback_emitted: usize,
        }

        let repr = Repr {
            inject_calls: self.inject_calls,
            inject_success: self.inject_success,
            final_segments_seen: self.final_segments_seen,
            fallback_stub_typed: self.fallback_stub_typed,
            shortcut_toggle_executed: self.shortcut_toggle_executed,
            shortcut_toggle_dropped: self.shortcut_toggle_dropped,
            pipeline_frames_in: self.pipeline_frames_in,
            pipeline_frames_resampled: self.pipeline_frames_resampled,
            pipeline_vad_windows: self.pipeline_vad_windows,
            pipeline_vad_speech: self.pipeline_vad_speech,
            pipeline_inference_calls: self.pipeline_inference_calls,
            pipeline_inference_errors: self.pipeline_inference_errors,
            pipeline_segments_emitted: self.pipeline_segments_emitted,
            pipeline_fallback_emitted: self.pipeline_fallback_emitted,
        };
        repr.serialize(serializer)
    }
}
