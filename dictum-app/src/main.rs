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
    inference::{stub::StubModel, ModelHandle},
    ipc::events::SegmentKind,
    DictumEngine,
};
use parking_lot::Mutex;
use settings::{
    apply_runtime_env_from_settings, apply_runtime_env_with_profile, default_settings_path,
    engine_config_for_settings, load_settings, resolve_app_profile, save_settings, RuntimeEnvMode,
};
use state::{AppState, PerfMetrics};
use storage::{HistoryRecordInput, LocalStore};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::ShortcutState;
use tracing::info;
use transform::{DictationMode, TextTransform};

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
        let is_minimized = window.is_minimized().unwrap_or(false);
        if is_visible && !is_minimized {
            let _ = window.hide();
        } else {
            reveal_main_window(app);
        }
    }
}

fn resolve_dictation_mode_for_app(
    settings: &settings::AppSettings,
    foreground_app: Option<&str>,
) -> DictationMode {
    if let Some(profile) = resolve_app_profile(settings, foreground_app) {
        return DictationMode::from_str(&profile.dictation_mode);
    }
    DictationMode::from_str(&settings.dictation_mode)
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
    dictation_mode: &str,
    active_profile_id: Option<&str>,
) -> (String, Vec<AppliedCorrection>) {
    let mut out = text.trim().to_string();
    if out.is_empty() || corrections.is_empty() {
        return (out, Vec::new());
    }
    let mut applied = Vec::new();
    for correction in corrections {
        if let Some(mode_affinity) = correction.mode_affinity.as_deref() {
            if !mode_affinity.eq_ignore_ascii_case(dictation_mode) {
                continue;
            }
        }
        if let Some(profile_affinity) = correction.app_profile_affinity.as_deref() {
            if active_profile_id != Some(profile_affinity) {
                continue;
            }
        }
        let heard = correction.heard.trim();
        let corrected = correction.corrected.trim();
        if heard.is_empty() || corrected.is_empty() {
            continue;
        }
        let replaced = replace_word_case_aware_local(&out, heard, corrected);
        if replaced != out {
            applied.push(AppliedCorrection {
                heard: correction.heard.clone(),
                corrected: correction.corrected.clone(),
                mode_affinity: correction.mode_affinity.clone(),
                app_profile_affinity: correction.app_profile_affinity.clone(),
            });
            out = replaced;
        }
    }
    (out, applied)
}

#[derive(Debug, Clone)]
struct AppliedCorrection {
    heard: String,
    corrected: String,
    mode_affinity: Option<String>,
    app_profile_affinity: Option<String>,
}

fn record_applied_correction_usage(
    settings: &Arc<Mutex<settings::AppSettings>>,
    learned_corrections: &Arc<parking_lot::RwLock<Vec<settings::LearnedCorrection>>>,
    settings_path: &std::path::Path,
    applied: &[AppliedCorrection],
) {
    if applied.is_empty() {
        return;
    }
    let used_at = chrono::Utc::now().to_rfc3339();
    let mut guard = settings.lock();
    let mut changed = false;
    for entry in applied {
        if let Some(existing) = guard.learned_corrections.iter_mut().find(|candidate| {
            candidate.heard.eq_ignore_ascii_case(&entry.heard)
                && candidate.corrected.eq_ignore_ascii_case(&entry.corrected)
                && candidate.mode_affinity == entry.mode_affinity
                && candidate.app_profile_affinity == entry.app_profile_affinity
        }) {
            // Real transcript-time usage should outrank manual setup-only saves.
            existing.hits = existing.hits.saturating_add(2);
            existing.last_used_at = Some(used_at.clone());
            changed = true;
        }
    }
    if !changed {
        return;
    }
    guard.learned_corrections.sort_by(|a, b| {
        b.hits
            .cmp(&a.hits)
            .then_with(|| b.last_used_at.cmp(&a.last_used_at))
            .then_with(|| a.heard.cmp(&b.heard))
    });
    guard.normalize();
    if let Err(error) = save_settings(settings_path, &guard) {
        tracing::warn!("failed to persist learned correction usage: {error}");
    }
    *learned_corrections.write() = guard.learned_corrections.clone();
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

#[cfg(test)]
mod tests {
    use super::apply_learned_corrections;
    use crate::settings::LearnedCorrection;

    #[test]
    fn applies_global_correction_without_context_filters() {
        let corrections = vec![LearnedCorrection {
            heard: "foo".into(),
            corrected: "bar".into(),
            hits: 1,
            mode_affinity: None,
            app_profile_affinity: None,
            last_used_at: None,
        }];

        let (text, applied) =
            apply_learned_corrections("foo test", &corrections, "conversation", None);

        assert_eq!(text, "bar test");
        assert_eq!(applied.len(), 1);
        assert_eq!(
            applied[0].heard, "foo",
            "applied correction should identify the matched rule"
        );
    }

    #[test]
    fn skips_correction_when_mode_affinity_does_not_match() {
        let corrections = vec![LearnedCorrection {
            heard: "printf".into(),
            corrected: "println!".into(),
            hits: 3,
            mode_affinity: Some("coding".into()),
            app_profile_affinity: None,
            last_used_at: None,
        }];

        let (text, applied) =
            apply_learned_corrections("printf ready", &corrections, "conversation", None);

        assert_eq!(text, "printf ready");
        assert!(applied.is_empty());
    }

    #[test]
    fn applies_profile_specific_correction_only_for_matching_profile() {
        let corrections = vec![LearnedCorrection {
            heard: "ship it".into(),
            corrected: "ShipIt".into(),
            hits: 5,
            mode_affinity: Some("command".into()),
            app_profile_affinity: Some("slack-profile".into()),
            last_used_at: None,
        }];

        let (text, applied) = apply_learned_corrections(
            "ship it now",
            &corrections,
            "command",
            Some("slack-profile"),
        );
        assert_eq!(text, "ShipIt now");
        assert_eq!(applied.len(), 1);

        let (text_miss, applied_miss) = apply_learned_corrections(
            "ship it now",
            &corrections,
            "command",
            Some("cursor-profile"),
        );
        assert_eq!(text_miss, "ship it now");
        assert!(applied_miss.is_empty());
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
    if app_settings.needs_persist_after_load() {
        if let Err(error) = save_settings(&settings_path, &app_settings) {
            tracing::warn!("failed to persist normalized settings on startup: {error}");
        } else {
            info!(
                loaded_schema_version = app_settings.loaded_schema_version,
                current_schema_version = settings::CURRENT_SETTINGS_SCHEMA_VERSION,
                migration_notes = ?app_settings.migration_notes,
                "persisted normalized settings on startup"
            );
        }
    }
    apply_runtime_env_from_settings(&app_settings, RuntimeEnvMode::FillMissing);
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

    let config = engine_config_for_settings(&app_settings);
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
    let duplicate_final_suppressed = Arc::new(AtomicUsize::new(0));
    let partial_rescues_used = Arc::new(AtomicUsize::new(0));
    let shortcut_toggle_inflight = Arc::new(AtomicBool::new(false));
    let shortcut_toggle_executed = Arc::new(AtomicUsize::new(0));
    let shortcut_toggle_dropped = Arc::new(AtomicUsize::new(0));
    let inject_calls_for_setup = Arc::clone(&inject_calls);
    let inject_success_for_setup = Arc::clone(&inject_success);
    let final_segments_seen_for_setup = Arc::clone(&final_segments_seen);
    let fallback_stub_typed_for_setup = Arc::clone(&fallback_stub_typed);
    let duplicate_final_suppressed_for_setup = Arc::clone(&duplicate_final_suppressed);
    let partial_rescues_used_for_setup = Arc::clone(&partial_rescues_used);
    let store_for_setup = Arc::clone(&store);
    let transformer_for_setup = Arc::clone(&transformer);
    let settings_for_setup = Arc::clone(&settings_state);
    let settings_path_for_setup = settings_path.clone();
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

            let shared_speech_end = Arc::new(Mutex::new(None::<Instant>));
            let shared_speech_end_for_activity = Arc::clone(&shared_speech_end);

            let mut transcript_rx = engine_for_setup.subscribe_transcripts();
            let handle1 = app_handle.clone();
            let inject_calls_clone = Arc::clone(&inject_calls_for_setup);
            let inject_success_clone = Arc::clone(&inject_success_for_setup);
            let final_segments_seen_clone = Arc::clone(&final_segments_seen_for_setup);
            let fallback_stub_typed_clone = Arc::clone(&fallback_stub_typed_for_setup);
            let duplicate_final_suppressed_clone =
                Arc::clone(&duplicate_final_suppressed_for_setup);
            let partial_rescues_used_clone = Arc::clone(&partial_rescues_used_for_setup);
            let store_clone = Arc::clone(&store_for_setup);
            let transformer_clone = Arc::clone(&transformer_for_setup);
            let settings_clone = Arc::clone(&settings_for_setup);
            let settings_path_clone = settings_path_for_setup.clone();
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
                            let mut applied_corrections = Vec::new();
                            let corrections_snapshot = learned_corrections_clone.read().clone();
                            let active_app = text_injector::foreground_process_name();
                            let (dictation_mode, active_profile_id) = {
                                let settings_guard = settings_clone.lock();
                                let active_profile =
                                    resolve_app_profile(&settings_guard, active_app.as_deref());
                                (
                                    resolve_dictation_mode_for_app(
                                        &settings_guard,
                                        active_app.as_deref(),
                                    ),
                                    active_profile.map(|profile| profile.id.clone()),
                                )
                            };
                            let transform_started = Instant::now();
                            for segment in event
                                .segments
                                .iter_mut()
                                .filter(|segment| segment.kind == SegmentKind::Final)
                            {
                                let (corrected_text, matched_corrections) = apply_learned_corrections(
                                    segment.text.trim(),
                                    &corrections_snapshot,
                                    dictation_mode.as_str(),
                                    active_profile_id.as_deref(),
                                );
                                let transformed =
                                    transformer_clone.apply(corrected_text.trim(), dictation_mode);
                                if !transformed.text.is_empty() {
                                    segment.text = transformed.text.clone();
                                    final_text_parts.push(transformed.text);
                                }
                                dictionary_applied |=
                                    transformed.dictionary_applied || !matched_corrections.is_empty();
                                snippet_applied |= transformed.snippet_applied;
                                applied_corrections.extend(matched_corrections);
                            }
                            record_applied_correction_usage(
                                &settings_clone,
                                &learned_corrections_clone,
                                &settings_path_clone,
                                &applied_corrections,
                            );
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
                                            partial_rescues_used_clone
                                                .fetch_add(1, Ordering::Relaxed);
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
                                        duplicate_final_suppressed_clone
                                            .fetch_add(1, Ordering::Relaxed);
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
                                    let latency_ms = shared_speech_end.lock().take().map(|t| t.elapsed().as_millis() as i64).unwrap_or(0);
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
                                            latency_ms,
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
                let mut was_speech = false;
                loop {
                    match activity_rx.recv().await {
                        Ok(event) => {
                            if was_speech && !event.is_speech {
                                *shared_speech_end_for_activity.lock() = Some(Instant::now());
                            }
                            was_speech = event.is_speech;
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

            let settings_for_profile_watcher = Arc::clone(&settings_for_setup);
            tauri::async_runtime::spawn(async move {
                let mut last_profile_key = String::new();
                loop {
                    let active_app = text_injector::foreground_process_name();
                    let profile_key = {
                        let settings_guard = settings_for_profile_watcher.lock();
                        let matched_profile =
                            resolve_app_profile(&settings_guard, active_app.as_deref());
                        let profile_key = matched_profile
                            .map(|profile| {
                                format!(
                                    "{}|{}|{}|{}",
                                    profile.id,
                                    profile.dictation_mode,
                                    profile.post_utterance_refine,
                                    profile.phrase_bias_terms.join("\n")
                                )
                            })
                            .unwrap_or_else(|| "base".to_string());
                        if profile_key != last_profile_key {
                            apply_runtime_env_with_profile(
                                &settings_guard,
                                matched_profile,
                                RuntimeEnvMode::Overwrite,
                            );
                            tracing::debug!(
                                active_app = active_app.as_deref().unwrap_or(""),
                                profile = profile_key,
                                "applied runtime env for active app profile"
                            );
                        }
                        profile_key
                    };
                    if profile_key != last_profile_key {
                        last_profile_key = profile_key;
                    }
                    tokio::time::sleep(Duration::from_millis(800)).await;
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
            duplicate_final_suppressed,
            partial_rescues_used,
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
            commands::prune_learned_corrections,
            commands::delete_learned_correction,
            commands::get_app_profiles,
            commands::upsert_app_profile,
            commands::delete_app_profile,
            commands::get_active_app_context,
            commands::get_perf_snapshot,
            commands::get_diagnostics_bundle,
            commands::export_diagnostics_bundle,
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
