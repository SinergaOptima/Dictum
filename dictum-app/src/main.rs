//! Dictum desktop application entry point.
//!
//! ## Runtime note
//!
//! Tauri v2 manages its own Tokio runtime internally.
//! We use `tauri::async_runtime::spawn` (not `tokio::spawn`) so our tasks
//! share Tauri's runtime and can safely call Tauri APIs.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod settings;
mod state;
mod storage;
mod text_injector;
mod transform;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use dictum_core::{
    engine::EngineConfig,
    inference::{stub::StubModel, ModelHandle},
    ipc::events::SegmentKind,
    DictumEngine,
};
use parking_lot::Mutex;
use settings::{apply_runtime_env_from_settings, default_settings_path, load_settings};
use state::AppState;
use storage::{HistoryRecordInput, LocalStore};
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::ShortcutState;
use tracing::info;
use transform::TextTransform;

const GLOBAL_TOGGLE_SHORTCUT: &str = "Ctrl+Shift+Space";

fn toggle_engine_from_shortcut<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let state = app.state::<AppState>();
    let engine = Arc::clone(&state.engine);
    let preferred_device = state.preferred_input_device.lock().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let should_start = !matches!(
            engine.status(),
            dictum_core::ipc::events::EngineStatus::Listening
        );

        let result = if should_start {
            engine.start_with_device(preferred_device)
        } else {
            engine.stop()
        };

        if let Err(e) = result {
            tracing::warn!("global shortcut toggle failed: {e}");
        }
    });
}

fn ensure_pill_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    if app.get_webview_window("pill").is_some() {
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(app, "pill", tauri::WebviewUrl::App("pill".into()))
        .title("Dictum Pill")
        .inner_size(152.0, 40.0)
        .min_inner_size(152.0, 40.0)
        .max_inner_size(152.0, 40.0)
        .resizable(false)
        .focused(false)
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .shadow(false)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .build()?;
    Ok(())
}

fn is_redacted_transcript(text: &str) -> bool {
    let mut total = 0usize;
    let mut stars = 0usize;
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        if c == '*' {
            stars += 1;
        }
    }
    total >= 6 && stars.saturating_mul(100) / total >= 80
}

fn is_duplicate_transcript(
    last: &Option<(String, Instant)>,
    text: &str,
    now: Instant,
    window: Duration,
) -> bool {
    if let Some((prev, at)) = last {
        prev == text && now.duration_since(*at) <= window
    } else {
        false
    }
}

fn main() {
    // ── Tracing ───────────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dictum=info".parse().unwrap()),
        )
        .init();

    info!("Dictum starting");

    let settings_path = default_settings_path();
    let app_settings = load_settings(&settings_path);
    apply_runtime_env_from_settings(&app_settings);
    info!(
        settings_path = ?settings_path,
        model_profile = %app_settings.model_profile,
        ort_ep = %app_settings.ort_ep,
        "runtime settings loaded"
    );

    // ── Engine setup ──────────────────────────────────────────────────────
    let model = {
        use dictum_core::inference::onnx::{OnnxModel, OnnxModelConfig};
        let cfg = OnnxModelConfig::default();
        if cfg.encoder_path.exists() && cfg.decoder_path.exists() && cfg.tokenizer_path.exists() {
            info!("loading OnnxModel from {:?}", cfg.encoder_path.parent());
            ModelHandle::new(OnnxModel::new(cfg))
        } else {
            tracing::warn!(
                "ONNX model files not found at {:?} — using StubModel",
                cfg.encoder_path.parent()
            );
            ModelHandle::new(StubModel::new())
        }
    };

    let mut config = EngineConfig::default();
    // Tune for global dictation: low-latency finalization plus stable long-form capture.
    config.vad_threshold = 0.0022;
    config.min_speech_samples = 800;
    // Force periodic finalization during very long speech while preserving continuity.
    config.max_speech_samples = 104_000;
    config.vad_hangover_frames = 4;
    // Partial inference during capture can starve the pipeline under long utterances.
    config.enable_partial_inference = false;
    config.silero_vad_threshold = 0.08;
    let engine = Arc::new(DictumEngine::new(config, model));

    // Warm up the model before Tauri starts.
    // StubModel is a no-op; OnnxModel loads sessions on this call.
    if let Err(e) = engine.warm_up() {
        tracing::error!("model warm-up failed: {e}");
    }

    let store = Arc::new(
        LocalStore::new(LocalStore::default_db_path())
            .expect("failed to initialize local encrypted storage"),
    );
    if let Err(e) = store.prune_history(app_settings.retention_days) {
        tracing::warn!("history prune failed at startup: {e}");
    }
    let transformer = Arc::new(TextTransform::new(Arc::clone(&store)));
    if let Err(e) = transformer.refresh() {
        tracing::warn!("failed to preload dictionary/snippets cache: {e}");
    }
    let settings_state = Arc::new(Mutex::new(app_settings.clone()));

    // ── Tauri app ─────────────────────────────────────────────────────────
    let engine_for_setup = Arc::clone(&engine);
    let toggle_debounce = Arc::new(Mutex::new(None::<Instant>));
    let toggle_debounce_for_handler = Arc::clone(&toggle_debounce);
    let global_shortcut_plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_shortcut(GLOBAL_TOGGLE_SHORTCUT)
        .expect("invalid global shortcut")
        .with_handler(move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let now = Instant::now();
                {
                    let mut guard = toggle_debounce_for_handler.lock();
                    if let Some(last) = *guard {
                        if now.duration_since(last) < Duration::from_millis(350) {
                            tracing::debug!(
                                "ignoring duplicate shortcut press within debounce window"
                            );
                            return;
                        }
                    }
                    *guard = Some(now);
                }
                toggle_engine_from_shortcut(app);
            }
        })
        .build();

    let inject_calls = Arc::new(AtomicUsize::new(0));
    let inject_success = Arc::new(AtomicUsize::new(0));
    let final_segments_seen = Arc::new(AtomicUsize::new(0));
    let fallback_stub_typed = Arc::new(AtomicUsize::new(0));
    let inject_calls_for_setup = Arc::clone(&inject_calls);
    let inject_success_for_setup = Arc::clone(&inject_success);
    let final_segments_seen_for_setup = Arc::clone(&final_segments_seen);
    let fallback_stub_typed_for_setup = Arc::clone(&fallback_stub_typed);
    let store_for_setup = Arc::clone(&store);
    let transformer_for_setup = Arc::clone(&transformer);
    let settings_for_setup = Arc::clone(&settings_state);

    tauri::Builder::default()
        .plugin(global_shortcut_plugin)
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // ── Forward engine events → Tauri event bus ───────────────────
            // Use tauri::async_runtime::spawn to share Tauri's Tokio runtime.

            let mut transcript_rx = engine_for_setup.subscribe_transcripts();
            let handle1 = app_handle.clone();
            let inject_calls_clone = Arc::clone(&inject_calls_for_setup);
            let inject_success_clone = Arc::clone(&inject_success_for_setup);
            let final_segments_seen_clone = Arc::clone(&final_segments_seen_for_setup);
            let fallback_stub_typed_clone = Arc::clone(&fallback_stub_typed_for_setup);
            let store_clone = Arc::clone(&store_for_setup);
            let transformer_clone = Arc::clone(&transformer_for_setup);
            let settings_clone = Arc::clone(&settings_for_setup);
            let mut last_injected_text: Option<(String, Instant)> = None;
            let mut last_partial_text: Option<(String, Instant)> = None;
            tauri::async_runtime::spawn(async move {
                loop {
                    match transcript_rx.recv().await {
                        Ok(mut event) => {
                            let partial_text = event
                                .segments
                                .iter()
                                .filter(|segment| segment.kind == SegmentKind::Partial)
                                .map(|segment| segment.text.trim())
                                .filter(|text| !text.is_empty())
                                .collect::<Vec<_>>()
                                .join(" ");
                            if !partial_text.is_empty() {
                                last_partial_text = Some((partial_text, Instant::now()));
                            }

                            let mut final_text_parts = Vec::new();
                            let mut dictionary_applied = false;
                            let mut snippet_applied = false;
                            for segment in event
                                .segments
                                .iter_mut()
                                .filter(|segment| segment.kind == SegmentKind::Final)
                            {
                                let transformed = transformer_clone.apply(segment.text.trim());
                                if !transformed.text.is_empty() {
                                    segment.text = transformed.text.clone();
                                    final_text_parts.push(transformed.text);
                                }
                                dictionary_applied |= transformed.dictionary_applied;
                                snippet_applied |= transformed.snippet_applied;
                            }

                            if let Err(e) = handle1.emit("dictum://transcript", &event) {
                                tracing::warn!("emit transcript: {e}");
                            }

                            let mut final_text = final_text_parts.join(" ");
                            let mut used_partial_rescue = false;
                            if !final_text.is_empty() {
                                final_segments_seen_clone.fetch_add(1, Ordering::Relaxed);
                                let mut should_inject_and_persist = true;
                                if is_redacted_transcript(&final_text) {
                                    should_inject_and_persist = false;
                                    tracing::warn!(
                                        "skipping injection for redacted transcript output"
                                    );
                                } else if final_text.eq_ignore_ascii_case("[speech captured]") {
                                    if let Some((partial, at)) = &last_partial_text {
                                        if at.elapsed() <= Duration::from_secs(10)
                                            && !partial.trim().is_empty()
                                            && !is_redacted_transcript(partial)
                                        {
                                            final_text = partial.trim().to_string();
                                            used_partial_rescue = true;
                                            tracing::warn!(
                                                "using recent partial transcript as fallback rescue for placeholder final segment"
                                            );
                                        } else {
                                            should_inject_and_persist = false;
                                            fallback_stub_typed_clone.fetch_add(1, Ordering::Relaxed);
                                            tracing::warn!(
                                                "skipping injection for placeholder fallback segment"
                                            );
                                        }
                                    } else {
                                        should_inject_and_persist = false;
                                        fallback_stub_typed_clone.fetch_add(1, Ordering::Relaxed);
                                        tracing::warn!(
                                            "skipping injection for placeholder fallback segment"
                                        );
                                    }
                                }

                                if should_inject_and_persist {
                                    let now = Instant::now();
                                    if is_duplicate_transcript(
                                        &last_injected_text,
                                        &final_text,
                                        now,
                                        Duration::from_millis(700),
                                    ) {
                                        tracing::warn!(
                                            "skipping duplicate final transcript within dedupe window"
                                        );
                                        continue;
                                    }
                                    let to_type = format!("{final_text} ");
                                    inject_calls_clone.fetch_add(1, Ordering::Relaxed);
                                    if let Err(e) = text_injector::inject_text(&to_type) {
                                        tracing::warn!("text injection failed: {e}");
                                    } else {
                                        inject_success_clone.fetch_add(1, Ordering::Relaxed);
                                        last_injected_text = Some((final_text.clone(), now));
                                    }
                                    let settings_guard = settings_clone.lock();
                                    if settings_guard.history_enabled {
                                        if let Err(e) = store_clone.insert_history(HistoryRecordInput {
                                            text: final_text.clone(),
                                            source: if settings_guard.cloud_opt_in {
                                                "hybrid".into()
                                            } else {
                                                "local".into()
                                            },
                                            latency_ms: 0,
                                            dictionary_applied,
                                            snippet_applied,
                                        }) {
                                            tracing::warn!("failed to persist history: {e}");
                                        }
                                    }
                                }
                                tracing::info!(
                                    transcript_seq = event.seq,
                                    final_text_len = final_text.len(),
                                    used_partial_rescue,
                                    inject_calls = inject_calls_clone.load(Ordering::Relaxed),
                                    inject_success = inject_success_clone.load(Ordering::Relaxed),
                                    "processed final transcript for text injection"
                                );
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("transcript receiver lagged by {n} events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            let mut status_rx = engine_for_setup.subscribe_status();
            let handle2 = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match status_rx.recv().await {
                        Ok(event) => {
                            if let Err(e) = handle2.emit("dictum://status", &event) {
                                tracing::warn!("emit status: {e}");
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("status receiver lagged by {n} events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            let mut activity_rx = engine_for_setup.subscribe_activity();
            let handle3 = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match activity_rx.recv().await {
                        Ok(event) => {
                            if let Err(e) = handle3.emit("dictum://activity", &event) {
                                tracing::warn!("emit activity: {e}");
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("activity receiver lagged by {n} events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            ensure_pill_window(&app_handle)?;

            Ok(())
        })
        .manage(AppState {
            engine: Arc::clone(&engine),
            preferred_input_device: Arc::new(Mutex::new(app_settings.preferred_input_device.clone())),
            inject_calls,
            inject_success,
            final_segments_seen,
            fallback_stub_typed,
            settings: Arc::clone(&settings_state),
            settings_path,
            store,
            transformer,
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_engine,
            commands::stop_engine,
            commands::get_status,
            commands::list_audio_devices,
            commands::set_preferred_input_device,
            commands::get_preferred_input_device,
            commands::get_runtime_settings,
            commands::set_runtime_settings,
            commands::get_privacy_settings,
            commands::set_privacy_settings,
            commands::get_history,
            commands::delete_history,
            commands::get_stats,
            commands::get_dictionary,
            commands::upsert_dictionary,
            commands::delete_dictionary,
            commands::get_snippets,
            commands::upsert_snippet,
            commands::delete_snippet,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
