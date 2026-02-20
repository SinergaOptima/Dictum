//! Tauri application state.
//!
//! `AppState` is managed via `app.manage(...)` and injected into command handlers
//! by Tauri's `State<'_, AppState>` extractor.

use dictum_core::DictumEngine;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use crate::settings::AppSettings;
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
    /// Persisted app settings cache.
    pub settings: Arc<Mutex<AppSettings>>,
    /// Absolute path to `settings.json`.
    pub settings_path: PathBuf,
    /// Local encrypted SQLite storage.
    pub store: Arc<LocalStore>,
    /// In-memory dictionary/snippet transform engine.
    pub transformer: Arc<TextTransform>,
}

impl AppState {
    pub fn diagnostics_snapshot(&self) -> AppDiagnostics {
        AppDiagnostics {
            inject_calls: self.inject_calls.load(Ordering::Relaxed),
            inject_success: self.inject_success.load(Ordering::Relaxed),
            final_segments_seen: self.final_segments_seen.load(Ordering::Relaxed),
            fallback_stub_typed: self.fallback_stub_typed.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AppDiagnostics {
    pub inject_calls: usize,
    pub inject_success: usize,
    pub final_segments_seen: usize,
    pub fallback_stub_typed: usize,
}
