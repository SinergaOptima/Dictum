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
mod model_profiles;
mod settings;
mod state;
mod storage;
mod text_injector;
mod transform;

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
use state::{AppState, PerfMetrics};
use storage::{HistoryRecordInput, LocalStore};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::ShortcutState;
use tracing::info;
use transform::TextTransform;

const DEFAULT_GLOBAL_TOGGLE_SHORTCUT: &str = "Ctrl+Shift+Space";
const TRAY_SHOW_HIDE_ID: &str = "tray_show_hide";
const TRAY_EXIT_ID: &str = "tray_exit";

#[cfg(target_os = "windows")]
fn enforce_single_instance() -> Option<isize> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE},
    };

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let mutex_name = to_wide("Global\\DictumSingleInstance");
    let mutex = unsafe { CreateMutexW(std::ptr::null(), true.into(), mutex_name.as_ptr()) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let window_title = to_wide("Dictum");
        let hwnd = unsafe { FindWindowW(std::ptr::null(), window_title.as_ptr()) };
        if !hwnd.is_null() {
            unsafe {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
        }
        return None;
    }
    Some(mutex as isize)
}

#[cfg(not(target_os = "windows"))]
fn enforce_single_instance() -> Option<isize> {
    Some(0)
}

fn apply_engine_profile(config: &mut EngineConfig, profile: &str) {
    match profile {
        "whisper_balanced_english" => {
            // Higher whisper sensitivity while keeping enough hangover for quiet tails.
            config.vad_threshold = 0.00125;
            config.min_speech_samples = 460;
            config.max_speech_samples = 96_000;
            config.vad_hangover_frames = 7;
            config.enable_partial_inference = true;
            config.silero_vad_threshold = 0.045;
        }
        "latency_short_utterance" => {
            config.vad_threshold = 0.00195;
            config.min_speech_samples = 420;
            config.max_speech_samples = 72_000;
            config.vad_hangover_frames = 3;
            config.enable_partial_inference = true;
            config.silero_vad_threshold = 0.065;
        }
        "balanced_general" => {
            config.vad_threshold = 0.0017;
            config.min_speech_samples = 600;
            config.max_speech_samples = 88_000;
            config.vad_hangover_frames = 4;
            config.enable_partial_inference = true;
            config.silero_vad_threshold = 0.058;
        }
        _ => {
            // stability_long_form (default)
            config.vad_threshold = 0.00145;
            config.min_speech_samples = 520;
            config.max_speech_samples = 104_000;
            config.vad_hangover_frames = 6;
            config.enable_partial_inference = true;
            config.silero_vad_threshold = 0.052;
        }
    }
}

fn toggle_engine_from_shortcut<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let state = app.state::<AppState>();
    let toggle_inflight = Arc::clone(&state.shortcut_toggle_inflight);
    if toggle_inflight
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        state
            .shortcut_toggle_dropped
            .fetch_add(1, Ordering::Relaxed);
        tracing::debug!("shortcut toggle dropped due to in-flight operation");
        return;
    }
    state
        .shortcut_toggle_executed
        .fetch_add(1, Ordering::Relaxed);
    let engine = Arc::clone(&state.engine);
    let preferred_device = state.preferred_input_device.lock().clone();
    let toggle_inflight_for_task = Arc::clone(&toggle_inflight);
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
        toggle_inflight_for_task.store(false, Ordering::SeqCst);
    });
}

fn ensure_pill_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    if app.get_webview_window("pill").is_some() {
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(app, "pill", tauri::WebviewUrl::App("pill".into()))
        .title("Dictum Pill")
        .inner_size(240.0, 60.0)
        .min_inner_size(240.0, 60.0)
        .max_inner_size(240.0, 60.0)
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

fn reveal_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn toggle_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let is_visible = window.is_visible().unwrap_or(false);
        if is_visible {
            let _ = window.hide();
        } else {
            reveal_main_window(app);
        }
    }
}

fn setup_system_tray<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let show_hide_item = MenuItem::with_id(
        app,
        TRAY_SHOW_HIDE_ID,
        "Show / Hide Dictum",
        true,
        None::<&str>,
    )?;
    let exit_item = MenuItem::with_id(app, TRAY_EXIT_ID, "Exit Dictum", true, None::<&str>)?;
    let tray_menu = Menu::with_items(app, &[&show_hide_item, &exit_item])?;

    let mut tray = TrayIconBuilder::with_id("dictum-tray")
        .menu(&tray_menu)
        .tooltip("Dictum")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id() == TRAY_SHOW_HIDE_ID {
                toggle_main_window(app);
            } else if event.id() == TRAY_EXIT_ID {
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
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

fn apply_learned_corrections(
    text: &str,
    corrections: &[settings::LearnedCorrection],
) -> (String, bool) {
    let mut out = text.trim().to_string();
    if out.is_empty() || corrections.is_empty() {
        return (out, false);
    }
    let mut applied = false;
    for correction in corrections {
        let heard = correction.heard.trim();
        let corrected = correction.corrected.trim();
        if heard.is_empty() || corrected.is_empty() {
            continue;
        }
        let replaced = replace_word_case_aware_local(&out, heard, corrected);
        if replaced != out {
            applied = true;
            out = replaced;
        }
    }
    (out, applied)
}

fn replace_word_case_aware_local(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || text.is_empty() {
        return text.to_string();
    }
    let needle_lower = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut changed = false;
    while i < chars.len() {
        let rem: String = chars[i..].iter().collect();
        if rem.to_ascii_lowercase().starts_with(&needle_lower) {
            let start_ok = if i == 0 {
                true
            } else {
                !is_word_char_local(chars[i - 1])
            };
            let end_idx = i + needle.chars().count();
            let end_ok = if end_idx >= chars.len() {
                true
            } else {
                !is_word_char_local(chars[end_idx])
            };
            if start_ok && end_ok {
                let source_slice: String = chars[i..end_idx].iter().collect();
                out.push_str(match_case_local(&source_slice, replacement).as_str());
                i = end_idx;
                changed = true;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    if changed {
        out
    } else {
        text.to_string()
    }
}

fn is_word_char_local(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

fn match_case_local(source: &str, replacement: &str) -> String {
    if source.chars().all(|c| c.is_uppercase()) {
        replacement.to_ascii_uppercase()
    } else if source
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
    {
        let mut chars = replacement.chars();
        if let Some(first) = chars.next() {
            format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
        } else {
            replacement.to_string()
        }
    } else {
        replacement.to_string()
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
    let _single_instance_guard = enforce_single_instance();
    if _single_instance_guard.is_none() {
        return;
    }

    let settings_path = default_settings_path();
    let app_settings = load_settings(&settings_path);
    apply_runtime_env_from_settings(&app_settings);
    info!(
        settings_path = ?settings_path,
        model_profile = %app_settings.model_profile,
        performance_profile = %app_settings.performance_profile,
        toggle_shortcut = %app_settings.toggle_shortcut,
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
    apply_engine_profile(&mut config, &app_settings.performance_profile);
    info!(
        performance_profile = %app_settings.performance_profile,
        vad_threshold = config.vad_threshold,
        min_speech_samples = config.min_speech_samples,
        max_speech_samples = config.max_speech_samples,
        vad_hangover_frames = config.vad_hangover_frames,
        enable_partial_inference = config.enable_partial_inference,
        silero_vad_threshold = config.silero_vad_threshold,
        "engine performance profile applied"
    );
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
        .with_shortcut(if app_settings.toggle_shortcut.trim().is_empty() {
            DEFAULT_GLOBAL_TOGGLE_SHORTCUT
        } else {
            app_settings.toggle_shortcut.as_str()
        })
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
    let shortcut_toggle_inflight = Arc::new(AtomicBool::new(false));
    let shortcut_toggle_executed = Arc::new(AtomicUsize::new(0));
    let shortcut_toggle_dropped = Arc::new(AtomicUsize::new(0));
    let inject_calls_for_setup = Arc::clone(&inject_calls);
    let inject_success_for_setup = Arc::clone(&inject_success);
    let final_segments_seen_for_setup = Arc::clone(&final_segments_seen);
    let fallback_stub_typed_for_setup = Arc::clone(&fallback_stub_typed);
    let store_for_setup = Arc::clone(&store);
    let transformer_for_setup = Arc::clone(&transformer);
    let settings_for_setup = Arc::clone(&settings_state);
    let learned_corrections_for_setup = Arc::new(parking_lot::RwLock::new(
        app_settings.learned_corrections.clone(),
    ));
    let learned_corrections_for_loop = Arc::clone(&learned_corrections_for_setup);
    let perf_metrics = Arc::new(Mutex::new(PerfMetrics::default()));
    let perf_metrics_for_setup = Arc::clone(&perf_metrics);

    tauri::Builder::default()
        .plugin(global_shortcut_plugin)
        .setup(move |app| {
            let app_handle = app.handle().clone();
            setup_system_tray(&app_handle)?;

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
            let learned_corrections_clone = Arc::clone(&learned_corrections_for_loop);
            let perf_metrics_clone = Arc::clone(&perf_metrics_for_setup);
            let mut last_injected_text: Option<(String, Instant)> = None;
            let mut last_partial_text: Option<(String, Instant)> = None;
            tauri::async_runtime::spawn(async move {
                let mut last_perf_log = Instant::now();
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
                            let corrections_snapshot = learned_corrections_clone.read().clone();
                            let transform_started = Instant::now();
                            for segment in event
                                .segments
                                .iter_mut()
                                .filter(|segment| segment.kind == SegmentKind::Final)
                            {
                                let (corrected_text, correction_applied) = apply_learned_corrections(
                                    segment.text.trim(),
                                    &corrections_snapshot,
                                );
                                let transformed = transformer_clone.apply(corrected_text.trim());
                                if !transformed.text.is_empty() {
                                    segment.text = transformed.text.clone();
                                    final_text_parts.push(transformed.text);
                                }
                                dictionary_applied |= transformed.dictionary_applied || correction_applied;
                                snippet_applied |= transformed.snippet_applied;
                            }
                            let transform_elapsed_ms = transform_started.elapsed().as_secs_f64() * 1000.0;
                            perf_metrics_clone
                                .lock()
                                .record_transform(transform_elapsed_ms);

                            if let Err(e) = handle1.emit("dictum://transcript", &event) {
                                tracing::warn!("emit transcript: {e}");
                            }

                            let mut final_text = final_text_parts.join(" ");
                            let mut used_partial_rescue = false;
                            if !final_text.is_empty() {
                                let finalize_started = Instant::now();
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
                                        let finalize_elapsed_ms =
                                            finalize_started.elapsed().as_secs_f64() * 1000.0;
                                        perf_metrics_clone
                                            .lock()
                                            .record_finalize(finalize_elapsed_ms);
                                        tracing::warn!(
                                            "skipping duplicate final transcript within dedupe window"
                                        );
                                        continue;
                                    }
                                    let to_type = format!("{final_text} ");
                                    inject_calls_clone.fetch_add(1, Ordering::Relaxed);
                                    let inject_started = Instant::now();
                                    if let Err(e) = text_injector::inject_text(&to_type) {
                                        tracing::warn!("text injection failed: {e}");
                                    } else {
                                        inject_success_clone.fetch_add(1, Ordering::Relaxed);
                                        last_injected_text = Some((final_text.clone(), now));
                                    }
                                    let inject_elapsed_ms =
                                        inject_started.elapsed().as_secs_f64() * 1000.0;
                                    perf_metrics_clone.lock().record_inject(inject_elapsed_ms);
                                    let settings_guard = settings_clone.lock();
                                    if settings_guard.history_enabled {
                                        let persist_started = Instant::now();
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
                                        let persist_elapsed_ms =
                                            persist_started.elapsed().as_secs_f64() * 1000.0;
                                        perf_metrics_clone
                                            .lock()
                                            .record_persist(persist_elapsed_ms);
                                    }
                                }
                                let finalize_elapsed_ms =
                                    finalize_started.elapsed().as_secs_f64() * 1000.0;
                                perf_metrics_clone
                                    .lock()
                                    .record_finalize(finalize_elapsed_ms);
                                tracing::info!(
                                    transcript_seq = event.seq,
                                    final_text_len = final_text.len(),
                                    used_partial_rescue,
                                    inject_calls = inject_calls_clone.load(Ordering::Relaxed),
                                    inject_success = inject_success_clone.load(Ordering::Relaxed),
                                    "processed final transcript for text injection"
                                );

                                if last_perf_log.elapsed() >= Duration::from_secs(10) {
                                    let snapshot = perf_metrics_clone.lock().snapshot();
                                    tracing::debug!(
                                        transform_p95_ms = snapshot.transform_ms.p95_ms,
                                        inject_p95_ms = snapshot.inject_ms.p95_ms,
                                        persist_p95_ms = snapshot.persist_ms.p95_ms,
                                        finalize_p95_ms = snapshot.finalize_ms.p95_ms,
                                        "perf snapshot"
                                    );
                                    last_perf_log = Instant::now();
                                }
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
            shortcut_toggle_inflight,
            shortcut_toggle_executed,
            shortcut_toggle_dropped,
            settings: Arc::clone(&settings_state),
            learned_corrections: Arc::clone(&learned_corrections_for_setup),
            settings_path,
            store,
            transformer,
            perf_metrics,
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_engine,
            commands::stop_engine,
            commands::get_status,
            commands::list_audio_devices,
            commands::set_preferred_input_device,
            commands::get_preferred_input_device,
            commands::get_runtime_settings,
            commands::get_model_profile_catalog,
            commands::get_model_profile_recommendation,
            commands::check_for_app_update,
            commands::download_and_install_app_update,
            commands::run_auto_tune,
            commands::run_benchmark_auto_tune,
            commands::set_runtime_settings,
            commands::get_learned_corrections,
            commands::learn_correction,
            commands::delete_learned_correction,
            commands::get_perf_snapshot,
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
