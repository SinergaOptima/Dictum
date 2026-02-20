//! Tauri command handlers.
//!
//! Each function is registered with `tauri::Builder::invoke_handler` and
//! callable from the frontend via `invoke(...)`.

use dictum_core::{audio::device::DeviceInfo, ipc::events::EngineStatus};
use tauri::State;
use tracing::info;

use crate::settings::{
    normalize_language_hint, normalize_model_profile, normalize_ort_ep, save_settings,
    RuntimeSettings,
};
use crate::state::AppState;
use crate::storage::{DictionaryEntry, HistoryPage, PrivacySettings, SnippetEntry, StatsPayload};

/// Start audio capture and the transcription pipeline.
#[tauri::command]
pub async fn start_engine(
    state: State<'_, AppState>,
    device_name: Option<String>,
) -> Result<(), String> {
    if let Some(name) = device_name {
        *state.preferred_input_device.lock() = Some(name);
    }
    let preferred = state.preferred_input_device.lock().clone();
    state
        .engine
        .start_with_device(preferred)
        .map_err(|e| e.to_string())
}

/// Stop audio capture and the pipeline.
#[tauri::command]
pub async fn stop_engine(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.stop().map_err(|e| e.to_string())?;
    let diag = state.diagnostics_snapshot();
    info!(
        inject_calls = diag.inject_calls,
        inject_success = diag.inject_success,
        final_segments_seen = diag.final_segments_seen,
        fallback_stub_typed = diag.fallback_stub_typed,
        "app diagnostics snapshot on stop"
    );
    Ok(())
}

/// Return the current engine status.
#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<EngineStatus, String> {
    Ok(state.engine.status())
}

/// Return a list of available audio input devices.
#[tauri::command]
pub async fn list_audio_devices(_state: State<'_, AppState>) -> Result<Vec<DeviceInfo>, String> {
    Ok(dictum_core::audio::device::list_input_devices())
}

/// Persist the preferred input device name used for future starts.
#[tauri::command]
pub async fn set_preferred_input_device(
    state: State<'_, AppState>,
    device_name: Option<String>,
) -> Result<(), String> {
    let normalized = device_name
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    *state.preferred_input_device.lock() = normalized.clone();

    let mut settings = state.settings.lock();
    settings.preferred_input_device = normalized;
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(())
}

/// Return the currently preferred input device name, if one is set.
#[tauri::command]
pub async fn get_preferred_input_device(
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    Ok(state.preferred_input_device.lock().clone())
}

/// Return persisted runtime settings for model profile and runtime configuration.
#[tauri::command]
pub async fn get_runtime_settings(state: State<'_, AppState>) -> Result<RuntimeSettings, String> {
    Ok(state.settings.lock().runtime_settings())
}

/// Persist runtime settings.
///
/// These settings are applied immediately for env-backed toggles and on next app
/// start for model/runtime reload.
#[tauri::command]
pub async fn set_runtime_settings(
    state: State<'_, AppState>,
    model_profile: Option<String>,
    ort_ep: Option<String>,
    language_hint: Option<String>,
    cloud_opt_in: Option<bool>,
    history_enabled: Option<bool>,
    retention_days: Option<usize>,
) -> Result<RuntimeSettings, String> {
    let mut settings = state.settings.lock();
    if let Some(profile) = model_profile {
        settings.model_profile = normalize_model_profile(&profile);
    }
    if let Some(ep) = ort_ep {
        settings.ort_ep = normalize_ort_ep(&ep);
    }
    if let Some(hint) = language_hint {
        settings.language_hint = normalize_language_hint(&hint);
    }
    if let Some(v) = cloud_opt_in {
        settings.cloud_opt_in = v;
    }
    if let Some(v) = history_enabled {
        settings.history_enabled = v;
    }
    if let Some(v) = retention_days {
        settings.retention_days = v.clamp(1, 3650);
    }

    std::env::set_var("DICTUM_ORT_EP", settings.ort_ep.clone());
    std::env::set_var("DICTUM_LANGUAGE_HINT", settings.language_hint.clone());
    std::env::set_var(
        "DICTUM_CLOUD_FALLBACK",
        if settings.cloud_opt_in { "1" } else { "0" },
    );

    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    state.store.prune_history(settings.retention_days)?;
    Ok(settings.runtime_settings())
}

#[tauri::command]
pub async fn get_privacy_settings(state: State<'_, AppState>) -> Result<PrivacySettings, String> {
    let settings = state.settings.lock();
    Ok(PrivacySettings {
        history_enabled: settings.history_enabled,
        retention_days: settings.retention_days,
        cloud_opt_in: settings.cloud_opt_in,
    })
}

#[tauri::command]
pub async fn set_privacy_settings(
    state: State<'_, AppState>,
    history_enabled: Option<bool>,
    retention_days: Option<usize>,
    cloud_opt_in: Option<bool>,
) -> Result<PrivacySettings, String> {
    let mut settings = state.settings.lock();
    if let Some(v) = history_enabled {
        settings.history_enabled = v;
    }
    if let Some(v) = retention_days {
        settings.retention_days = v.clamp(1, 3650);
    }
    if let Some(v) = cloud_opt_in {
        settings.cloud_opt_in = v;
    }
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    state.store.prune_history(settings.retention_days)?;
    std::env::set_var(
        "DICTUM_CLOUD_FALLBACK",
        if settings.cloud_opt_in { "1" } else { "0" },
    );
    Ok(PrivacySettings {
        history_enabled: settings.history_enabled,
        retention_days: settings.retention_days,
        cloud_opt_in: settings.cloud_opt_in,
    })
}

#[tauri::command]
pub async fn get_history(
    state: State<'_, AppState>,
    page: Option<usize>,
    page_size: Option<usize>,
    query: Option<String>,
) -> Result<HistoryPage, String> {
    state
        .store
        .get_history(page.unwrap_or(1), page_size.unwrap_or(50), query)
}

#[tauri::command]
pub async fn delete_history(
    state: State<'_, AppState>,
    ids: Option<Vec<String>>,
    older_than_days: Option<usize>,
) -> Result<usize, String> {
    state.store.delete_history(ids, older_than_days)
}

#[tauri::command]
pub async fn get_stats(
    state: State<'_, AppState>,
    range_days: Option<usize>,
) -> Result<StatsPayload, String> {
    state.store.get_stats(range_days.unwrap_or(30))
}

#[tauri::command]
pub async fn get_dictionary(state: State<'_, AppState>) -> Result<Vec<DictionaryEntry>, String> {
    state.store.list_dictionary()
}

#[tauri::command]
pub async fn upsert_dictionary(
    state: State<'_, AppState>,
    entry: DictionaryEntry,
) -> Result<DictionaryEntry, String> {
    let updated = state.store.upsert_dictionary(entry)?;
    state.transformer.refresh()?;
    Ok(updated)
}

#[tauri::command]
pub async fn delete_dictionary(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.store.delete_dictionary(&id)?;
    state.transformer.refresh()?;
    Ok(())
}

#[tauri::command]
pub async fn get_snippets(state: State<'_, AppState>) -> Result<Vec<SnippetEntry>, String> {
    state.store.list_snippets()
}

#[tauri::command]
pub async fn upsert_snippet(
    state: State<'_, AppState>,
    entry: SnippetEntry,
) -> Result<SnippetEntry, String> {
    let updated = state.store.upsert_snippet(entry)?;
    state.transformer.refresh()?;
    Ok(updated)
}

#[tauri::command]
pub async fn delete_snippet(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.store.delete_snippet(&id)?;
    state.transformer.refresh()?;
    Ok(())
}
