//! Tauri command handlers.
//!
//! Each function is registered with `tauri::Builder::invoke_handler` and
//! callable from the frontend via `invoke(...)`.

use dictum_core::{audio::device::DeviceInfo, ipc::events::EngineStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use tauri::State;
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tracing::{info, warn};

use crate::model_profiles::{
    model_profile_catalog, recommend_model_profile, ModelProfileMetadata,
    ModelProfileRecommendation,
};
use crate::settings::{
    normalize_cloud_mode, normalize_language_hint, normalize_model_profile, normalize_ort_ep,
    normalize_dictation_mode, normalize_performance_profile, normalize_toggle_shortcut,
    resolve_app_profile, save_settings, sync_runtime_with_settings, AppProfile,
    LearnedCorrection, RuntimeEnvMode, RuntimeSettings,
};
use crate::state::{AppState, PerfSnapshot};
use crate::storage::{
    DictionaryEntry, HistoryPage, HistoryStorageSummary, PrivacySettings, SnippetEntry,
    StatsPayload,
};
use crate::text_injector;

const DEFAULT_UPDATE_REPO_SLUG: &str = "sinergaoptima/dictum";
const LEGACY_UPDATE_REPO_SLUGS: &[&str] = &["latticelabs/dictum"];
const UPDATE_TIMEOUT_SECS: u64 = 45;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTuneResult {
    pub runtime_settings: RuntimeSettings,
    pub summary: String,
    pub cpu_threads: usize,
    pub ort_intra_threads: usize,
    pub ort_inter_threads: usize,
    pub ort_parallel: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkAutoTuneResult {
    pub runtime_settings: RuntimeSettings,
    pub summary: String,
    pub measured_finalize_p95_ms: f64,
    pub measured_fallback_rate_pct: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub repo_slug: String,
    pub release_name: Option<String>,
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
    pub html_url: String,
    pub asset_name: Option<String>,
    pub asset_download_url: Option<String>,
    pub checksum_asset_name: Option<String>,
    pub checksum_asset_download_url: Option<String>,
    pub expected_installer_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsBundle {
    pub generated_at: String,
    pub app_version: String,
    pub update_repo_slug: String,
    pub settings_path: String,
    pub active_app_context: ActiveAppContext,
    pub runtime_settings: RuntimeSettings,
    pub privacy_settings: PrivacySettings,
    pub perf_snapshot: PerfSnapshot,
    pub history_storage: HistoryStorageSummary,
    pub devices: Vec<DeviceInfo>,
    pub correction_diagnostics: CorrectionDiagnostics,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExportResult {
    pub path: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionRuleSummary {
    pub heard: String,
    pub corrected: String,
    pub hits: usize,
    pub mode_affinity: Option<String>,
    pub app_profile_affinity: Option<String>,
    pub app_profile_name: Option<String>,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionDiagnostics {
    pub total_rules: usize,
    pub global_rules: usize,
    pub mode_scoped_rules: usize,
    pub profile_scoped_rules: usize,
    pub unused_rules: usize,
    pub orphaned_profile_rules: usize,
    pub stale_rules: usize,
    pub top_rules: Vec<CorrectionRuleSummary>,
    pub recent_rules: Vec<CorrectionRuleSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionPruneResult {
    pub rules: Vec<LearnedCorrection>,
    pub removed_unused: usize,
    pub removed_orphaned_profiles: usize,
    pub removed_stale: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAppContext {
    pub foreground_app: Option<String>,
    pub matched_profile_id: Option<String>,
    pub matched_profile_name: Option<String>,
    pub dictation_mode: String,
    pub phrase_bias_term_count: usize,
    pub post_utterance_refine: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

fn normalize_repo_slug(input: Option<String>) -> Result<String, String> {
    let chosen = input
        .or_else(|| std::env::var("DICTUM_UPDATE_REPO").ok())
        .unwrap_or_else(|| DEFAULT_UPDATE_REPO_SLUG.to_string());
    let raw_slug = chosen.trim().trim_matches('/').to_ascii_lowercase();
    let slug = if LEGACY_UPDATE_REPO_SLUGS
        .iter()
        .any(|legacy| raw_slug.eq_ignore_ascii_case(legacy))
    {
        DEFAULT_UPDATE_REPO_SLUG.to_string()
    } else {
        raw_slug
    };
    if slug.is_empty() {
        return Err("Update repository cannot be empty.".into());
    }
    let mut parts = slug.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return Err("Update repository must be in the form owner/repo.".into());
    }
    let valid = |value: &str| {
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    };
    if !valid(owner) || !valid(repo) {
        return Err("Update repository contains invalid characters.".into());
    }
    Ok(format!("{owner}/{repo}"))
}

fn version_tuple(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().trim_start_matches('v').trim_start_matches('V');
    let core = trimmed.split(['-', '+']).next()?.trim();
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

fn sanitize_filename(raw: &str) -> String {
    let mut cleaned = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if cleaned.is_empty() {
        cleaned = "dictum-update.exe".into();
    }
    if !cleaned.to_ascii_lowercase().ends_with(".exe") {
        cleaned.push_str(".exe");
    }
    cleaned
}

fn select_installer_asset(assets: &[GitHubAsset]) -> Option<GitHubAsset> {
    let pick = |predicate: &dyn Fn(&str) -> bool| {
        assets
            .iter()
            .find(|asset| predicate(&asset.name.to_ascii_lowercase()))
            .cloned()
    };
    pick(&|name| name.ends_with("-setup.exe"))
        .or_else(|| pick(&|name| name.contains("setup") && name.ends_with(".exe")))
        .or_else(|| pick(&|name| name.ends_with(".msi")))
        .or_else(|| pick(&|name| name.ends_with(".exe")))
}

fn select_checksums_asset(assets: &[GitHubAsset]) -> Option<GitHubAsset> {
    assets
        .iter()
        .find(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name == "sha256sums.txt" || name.ends_with("/sha256sums.txt")
        })
        .cloned()
}

fn normalize_sha256_hex(raw: &str) -> Option<String> {
    let candidate = raw.trim().trim_start_matches('*').to_ascii_lowercase();
    if candidate.len() == 64 && candidate.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(candidate)
    } else {
        None
    }
}

fn parse_sha256_from_sums(contents: &str, file_name: &str) -> Option<String> {
    let target = file_name.trim().to_ascii_lowercase();
    let target_basename = std::path::Path::new(&target)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(target.as_str())
        .to_string();

    for line in contents.lines() {
        let raw = line.trim();
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        let parts = raw.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }

        let maybe_hash_first = normalize_sha256_hex(parts[0]);
        let maybe_hash_second = normalize_sha256_hex(parts[1]);
        let (name_part, hash_part) = if let Some(hash) = maybe_hash_first {
            (parts[1], hash)
        } else if let Some(hash) = maybe_hash_second {
            (parts[0], hash)
        } else {
            continue;
        };

        let name_normalized = name_part
            .trim()
            .trim_start_matches('*')
            .to_ascii_lowercase();
        let name_basename = std::path::Path::new(&name_normalized)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(name_normalized.as_str())
            .to_string();
        if name_normalized == target || name_basename == target_basename {
            return Some(hash_part);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn verify_windows_authenticode(path: &std::path::Path) -> Result<String, String> {
    let escaped = path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$status=(Get-AuthenticodeSignature -FilePath '{escaped}').Status.ToString(); Write-Output $status"
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("failed to run Authenticode verification: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "Authenticode verification command failed{}",
            if stderr.is_empty() {
                "".to_string()
            } else {
                format!(": {stderr}")
            }
        ));
    }
    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if status.eq_ignore_ascii_case("Valid") {
        Ok(status)
    } else {
        Err(format!(
            "installer signature status is '{status}', expected 'Valid'"
        ))
    }
}

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

/// Return model profile metadata catalog for UX guidance.
#[tauri::command]
pub async fn get_model_profile_catalog() -> Result<Vec<ModelProfileMetadata>, String> {
    Ok(model_profile_catalog())
}

/// Return best-effort hardware-based model profile recommendation.
#[tauri::command]
pub async fn get_model_profile_recommendation(
    state: State<'_, AppState>,
) -> Result<ModelProfileRecommendation, String> {
    let ort_ep = state.settings.lock().ort_ep.clone();
    Ok(recommend_model_profile(&ort_ep))
}

/// Check GitHub Releases for an available app update.
#[tauri::command]
pub async fn check_for_app_update(
    app: tauri::AppHandle,
    repo_slug: Option<String>,
) -> Result<AppUpdateInfo, String> {
    let repo_slug = normalize_repo_slug(repo_slug)?;
    let current_version = app.package_info().version.to_string();
    let current_version_for_check = current_version.clone();
    let check_url = format!("https://api.github.com/repos/{repo_slug}/releases/latest");
    let release = tauri::async_runtime::spawn_blocking(move || -> Result<GitHubRelease, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(UPDATE_TIMEOUT_SECS))
            .user_agent(format!("Dictum/{current_version_for_check} (update-check)"))
            .build()
            .map_err(|e| format!("failed to build update client: {e}"))?;
        let response = client
            .get(&check_url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .map_err(|e| format!("failed to check release feed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "update feed returned HTTP {}",
                response.status().as_u16()
            ));
        }
        response
            .json::<GitHubRelease>()
            .map_err(|e| format!("failed to parse release feed: {e}"))
    })
    .await
    .map_err(|e| format!("update check task failed: {e}"))??;

    if release.draft {
        return Err("Latest GitHub release is still marked as draft.".into());
    }

    let latest_version = release
        .tag_name
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string();
    let has_update = match (
        version_tuple(&current_version),
        version_tuple(&latest_version),
    ) {
        (Some(current), Some(latest)) => latest > current,
        _ => current_version.trim() != latest_version.trim(),
    } && !release.prerelease;

    let installer_asset = select_installer_asset(&release.assets);
    let checksums_asset = select_checksums_asset(&release.assets);
    let expected_installer_sha256 = if let (Some(installer), Some(checksums)) =
        (installer_asset.as_ref(), checksums_asset.as_ref())
    {
        let checksums_url = checksums.browser_download_url.clone();
        let installer_name = installer.name.clone();
        let version_for_hash_lookup = current_version.clone();
        match tauri::async_runtime::spawn_blocking(move || -> Result<Option<String>, String> {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(UPDATE_TIMEOUT_SECS))
                .user_agent(format!(
                    "Dictum/{version_for_hash_lookup} (update-checksum-fetch)"
                ))
                .build()
                .map_err(|e| format!("failed to build checksum client: {e}"))?;
            let response = client
                .get(&checksums_url)
                .header(reqwest::header::ACCEPT, "text/plain")
                .send()
                .map_err(|e| format!("failed to download checksum manifest: {e}"))?;
            if !response.status().is_success() {
                return Err(format!(
                    "checksum manifest returned HTTP {}",
                    response.status().as_u16()
                ));
            }
            let body = response
                .text()
                .map_err(|e| format!("failed to parse checksum manifest: {e}"))?;
            Ok(parse_sha256_from_sums(&body, &installer_name))
        })
        .await
        {
            Ok(Ok(found)) => found,
            Ok(Err(e)) => {
                warn!("update checksum fetch failed: {e}");
                None
            }
            Err(e) => {
                warn!("checksum fetch task failed: {e}");
                None
            }
        }
    } else {
        None
    };
    Ok(AppUpdateInfo {
        current_version,
        latest_version,
        has_update,
        repo_slug,
        release_name: release.name,
        release_notes: release.body,
        published_at: release.published_at,
        html_url: release.html_url,
        asset_name: installer_asset.as_ref().map(|asset| asset.name.clone()),
        asset_download_url: installer_asset.map(|asset| asset.browser_download_url),
        checksum_asset_name: checksums_asset.as_ref().map(|asset| asset.name.clone()),
        checksum_asset_download_url: checksums_asset.map(|asset| asset.browser_download_url),
        expected_installer_sha256,
    })
}

/// Download and launch an installer for an available app update.
#[tauri::command]
pub async fn download_and_install_app_update(
    app: tauri::AppHandle,
    download_url: String,
    asset_name: Option<String>,
    silent_install: Option<bool>,
    auto_exit: Option<bool>,
    expected_sha256: Option<String>,
) -> Result<String, String> {
    let url = download_url.trim().to_string();
    if !url.starts_with("https://") {
        return Err("Update URL must use HTTPS.".into());
    }
    let suggested_name = asset_name
        .unwrap_or_else(|| {
            url.rsplit('/')
                .next()
                .map(str::to_string)
                .unwrap_or_else(|| "dictum-update.exe".into())
        })
        .trim()
        .to_string();
    let file_name = sanitize_filename(&suggested_name);
    let silent_install = silent_install.unwrap_or(true);
    let expected_sha256 = expected_sha256
        .as_deref()
        .and_then(normalize_sha256_hex)
        .ok_or_else(|| "Missing expected SHA-256 checksum for installer.".to_string())?;
    let install_path = tauri::async_runtime::spawn_blocking(
        move || -> Result<(std::path::PathBuf, String), String> {
            let updates_dir = std::env::temp_dir().join("dictum-updates");
            std::fs::create_dir_all(&updates_dir)
                .map_err(|e| format!("failed to prepare update directory: {e}"))?;
            let target_path = updates_dir.join(file_name);

            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(UPDATE_TIMEOUT_SECS * 4))
                .user_agent("Dictum updater installer downloader")
                .build()
                .map_err(|e| format!("failed to build download client: {e}"))?;
            let mut response = client
                .get(&url)
                .header(reqwest::header::ACCEPT, "application/octet-stream")
                .send()
                .map_err(|e| format!("failed to download update installer: {e}"))?;
            if !response.status().is_success() {
                return Err(format!(
                    "installer download returned HTTP {}",
                    response.status().as_u16()
                ));
            }
            let mut file = std::fs::File::create(&target_path)
                .map_err(|e| format!("failed to create installer file: {e}"))?;
            let mut hasher = Sha256::new();
            let mut buf = [0u8; 64 * 1024];
            loop {
                let read = std::io::Read::read(&mut response, &mut buf)
                    .map_err(|e| format!("failed while reading installer payload: {e}"))?;
                if read == 0 {
                    break;
                }
                std::io::Write::write_all(&mut file, &buf[..read])
                    .map_err(|e| format!("failed while writing installer payload: {e}"))?;
                hasher.update(&buf[..read]);
            }
            std::io::Write::flush(&mut file)
                .map_err(|e| format!("failed to flush installer payload: {e}"))?;
            let size = std::fs::metadata(&target_path)
                .map_err(|e| format!("failed to verify installer file: {e}"))?
                .len();
            if size == 0 {
                return Err("installer download was empty".into());
            }
            let actual_sha256 = format!("{:x}", hasher.finalize());
            if actual_sha256 != expected_sha256 {
                let _ = std::fs::remove_file(&target_path);
                return Err(format!(
                    "installer checksum mismatch (expected {expected_sha256}, got {actual_sha256})"
                ));
            }
            Ok((target_path, actual_sha256))
        },
    )
    .await
    .map_err(|e| format!("installer download task failed: {e}"))??;
    let (install_path, actual_sha256) = install_path;

    #[cfg(target_os = "windows")]
    {
        let signature_status = verify_windows_authenticode(&install_path)?;
        let mut command = std::process::Command::new(&install_path);
        if silent_install {
            command.arg("/S");
        }
        command
            .spawn()
            .map_err(|e| format!("failed to launch installer: {e}"))?;
        info!(
            installer = %install_path.display(),
            sha256 = %actual_sha256,
            signature_status = %signature_status,
            "verified and launched update installer"
        );
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("In-app installer launch is currently implemented for Windows only.".into());
    }

    if auto_exit.unwrap_or(false) {
        app.exit(0);
    }

    Ok(format!(
        "Installer verified (sha256 {}) and launched from '{}'.",
        actual_sha256,
        install_path.display(),
    ))
}

/// Run one-shot hardware-aware auto tuning and persist applied runtime defaults.
#[tauri::command]
pub async fn run_auto_tune(state: State<'_, AppState>) -> Result<AutoTuneResult, String> {
    let cpu_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);

    let mut settings = state.settings.lock();
    let recommendation = recommend_model_profile(&settings.ort_ep);
    let prefers_gpu = recommendation.suggested_ort_ep == "directml";
    let intra_threads = if prefers_gpu {
        cpu_threads.clamp(4, 12)
    } else {
        cpu_threads.clamp(2, 12)
    };
    let inter_threads = if prefers_gpu { 2 } else { 1 };
    let ort_parallel = prefers_gpu && cpu_threads >= 8;
    let performance_profile = if cpu_threads >= 16 {
        "whisper_balanced_english"
    } else if cpu_threads >= 10 {
        "balanced_general"
    } else {
        "latency_short_utterance"
    };

    settings.model_profile = normalize_model_profile(&recommendation.recommended_profile);
    settings.ort_ep = normalize_ort_ep(&recommendation.suggested_ort_ep);
    settings.ort_intra_threads = intra_threads;
    settings.ort_inter_threads = inter_threads;
    settings.ort_parallel = ort_parallel;
    settings.performance_profile = normalize_performance_profile(performance_profile);
    settings.normalize();
    sync_runtime_with_settings(state.engine.as_ref(), &settings, RuntimeEnvMode::Overwrite);

    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    let runtime = settings.runtime_settings();
    Ok(AutoTuneResult {
        runtime_settings: runtime,
        summary: format!(
            "Applied {} with {} EP and ONNX threads intra/inter={}/{} (parallel={}).",
            settings.model_profile,
            settings.ort_ep,
            settings.ort_intra_threads,
            settings.ort_inter_threads,
            settings.ort_parallel
        ),
        cpu_threads,
        ort_intra_threads: settings.ort_intra_threads,
        ort_inter_threads: settings.ort_inter_threads,
        ort_parallel: settings.ort_parallel,
    })
}

/// Run benchmark-guided auto tuning using measured room/voice/perf metrics.
#[tauri::command]
pub async fn run_benchmark_auto_tune(
    state: State<'_, AppState>,
    ambient_p90: f32,
    whisper_p70: f32,
    normal_p80: f32,
    finalize_p95_ms: f64,
    fallback_rate_pct: f32,
) -> Result<BenchmarkAutoTuneResult, String> {
    let cpu_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);

    let mut settings = state.settings.lock();
    let recommendation = recommend_model_profile(&settings.ort_ep);
    let prefers_gpu = recommendation.suggested_ort_ep == "directml";

    settings.model_profile = normalize_model_profile(&recommendation.recommended_profile);
    settings.ort_ep = normalize_ort_ep(&recommendation.suggested_ort_ep);

    // Thread tuning informed by measured finalize latency (not only hardware).
    let mut intra_threads = if prefers_gpu {
        cpu_threads.clamp(4, 14)
    } else {
        cpu_threads.clamp(2, 12)
    };
    let mut inter_threads = if prefers_gpu { 2usize } else { 1usize };
    let mut ort_parallel = prefers_gpu && cpu_threads >= 8;
    if finalize_p95_ms > 420.0 {
        intra_threads = (intra_threads + 2).clamp(2, 16);
        inter_threads = 1;
        ort_parallel = true;
    } else if finalize_p95_ms < 150.0 {
        intra_threads = intra_threads.saturating_sub(1).clamp(2, 16);
        inter_threads = inter_threads.clamp(1, 2);
    }
    settings.ort_intra_threads = intra_threads;
    settings.ort_inter_threads = inter_threads;
    settings.ort_parallel = ort_parallel;

    settings.performance_profile = if fallback_rate_pct >= 20.0 {
        "stability_long_form".into()
    } else if finalize_p95_ms >= 320.0 {
        "latency_short_utterance".into()
    } else {
        "whisper_balanced_english".into()
    };

    // Voice/room-tuned activity + gain settings.
    let ambient_p90 = ambient_p90.clamp(0.0, 0.2);
    let whisper_p70 = whisper_p70.clamp(0.0, 0.4);
    let normal_p80 = normal_p80.clamp(0.0, 0.8);
    let noise_gate = (ambient_p90 * 1.45).clamp(0.0004, 0.03);
    let activity_sensitivity = (0.34 / (whisper_p70 - noise_gate).max(0.0001)).clamp(1.0, 20.0);
    let pill_sensitivity = (activity_sensitivity * 1.12).clamp(1.0, 20.0);
    let input_gain_boost = (0.02 / whisper_p70.max(0.0001)).clamp(0.5, 8.0);
    let clip_threshold =
        (normal_p80.max(ambient_p90 * 12.0).max(whisper_p70 * 8.0)).clamp(0.12, 0.95);

    settings.activity_noise_gate = noise_gate;
    settings.activity_sensitivity = activity_sensitivity;
    settings.pill_visualizer_sensitivity = pill_sensitivity;
    settings.input_gain_boost = input_gain_boost;
    settings.activity_clip_threshold = clip_threshold;
    settings.normalize();
    sync_runtime_with_settings(state.engine.as_ref(), &settings, RuntimeEnvMode::Overwrite);

    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    let runtime = settings.runtime_settings();
    let summary = format!(
        "Benchmark tune applied: model={}, perf={}, intra/inter={}/{}, p95={:.0}ms, fallback={:.1}%",
        settings.model_profile,
        settings.performance_profile,
        settings.ort_intra_threads,
        settings.ort_inter_threads,
        finalize_p95_ms,
        fallback_rate_pct
    );

    Ok(BenchmarkAutoTuneResult {
        runtime_settings: runtime,
        summary,
        measured_finalize_p95_ms: finalize_p95_ms,
        measured_fallback_rate_pct: fallback_rate_pct,
    })
}

/// Return learned correction rules (`heard` -> `corrected`).
#[tauri::command]
pub async fn get_learned_corrections(
    state: State<'_, AppState>,
) -> Result<Vec<LearnedCorrection>, String> {
    Ok(state.settings.lock().learned_corrections.clone())
}

/// Teach a correction pair used for live transcript cleanup.
#[tauri::command]
pub async fn learn_correction(
    state: State<'_, AppState>,
    heard: String,
    corrected: String,
    mode_affinity: Option<String>,
    app_profile_affinity: Option<String>,
) -> Result<Vec<LearnedCorrection>, String> {
    let heard = heard.trim().to_ascii_lowercase();
    let corrected = corrected.trim().to_string();
    let mode_affinity = mode_affinity
        .as_ref()
        .map(|value| crate::settings::normalize_dictation_mode(value))
        .filter(|value| !value.is_empty());
    let app_profile_affinity = app_profile_affinity
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if heard.is_empty() || corrected.is_empty() {
        return Err("Both 'heard' and 'corrected' are required.".into());
    }

    let mut settings = state.settings.lock();
    if let Some(profile_id) = app_profile_affinity.as_deref() {
        if !settings.app_profiles.iter().any(|profile| profile.id == profile_id) {
            return Err("The selected app profile no longer exists. Save this correction as Global or Mode, or recreate the profile.".into());
        }
    }
    if let Some(existing) = settings
        .learned_corrections
        .iter_mut()
        .find(|c| {
            c.heard.eq_ignore_ascii_case(&heard)
                && c.mode_affinity == mode_affinity
                && c.app_profile_affinity == app_profile_affinity
        })
    {
        existing.corrected = corrected.clone();
        existing.hits = existing.hits.saturating_add(1);
        existing.mode_affinity = mode_affinity.clone();
        existing.app_profile_affinity = app_profile_affinity.clone();
        existing.last_used_at = Some(chrono::Utc::now().to_rfc3339());
    } else {
        settings.learned_corrections.push(LearnedCorrection {
            heard: heard.clone(),
            corrected: corrected.clone(),
            hits: 1,
            mode_affinity: mode_affinity.clone(),
            app_profile_affinity: app_profile_affinity.clone(),
            last_used_at: Some(chrono::Utc::now().to_rfc3339()),
        });
    }
    settings
        .learned_corrections
        .sort_by(|a, b| {
            b.hits
                .cmp(&a.hits)
                .then_with(|| b.last_used_at.cmp(&a.last_used_at))
                .then_with(|| a.heard.cmp(&b.heard))
        });
    settings.normalize();
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;

    let updated = settings.learned_corrections.clone();
    drop(settings);
    *state.learned_corrections.write() = updated.clone();
    Ok(updated)
}

/// Remove stale, unused, or orphaned learned correction rules in one pass.
#[tauri::command]
pub async fn prune_learned_corrections(
    state: State<'_, AppState>,
    remove_unused: Option<bool>,
    remove_orphaned_profiles: Option<bool>,
    remove_stale: Option<bool>,
) -> Result<CorrectionPruneResult, String> {
    let remove_unused = remove_unused.unwrap_or(false);
    let remove_orphaned_profiles = remove_orphaned_profiles.unwrap_or(false);
    let remove_stale = remove_stale.unwrap_or(false);
    if !remove_unused && !remove_orphaned_profiles && !remove_stale {
        return Err("Select at least one correction prune filter.".into());
    }

    let mut settings = state.settings.lock();
    let valid_profile_ids = settings
        .app_profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut removed_unused_count = 0usize;
    let mut removed_orphaned_count = 0usize;
    let mut removed_stale_count = 0usize;
    settings.learned_corrections.retain(|rule| {
        let is_unused = remove_unused && rule_is_unused(rule);
        let is_orphaned = remove_orphaned_profiles
            && rule
                .app_profile_affinity
                .as_ref()
                .is_some_and(|profile_id| !valid_profile_ids.contains(profile_id));
        let is_stale = remove_stale && rule_is_stale(rule);
        if is_unused {
            removed_unused_count += 1;
        }
        if is_orphaned {
            removed_orphaned_count += 1;
        }
        if is_stale {
            removed_stale_count += 1;
        }
        !(is_unused || is_orphaned || is_stale)
    });
    settings.normalize();
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;

    let updated = settings.learned_corrections.clone();
    drop(settings);
    *state.learned_corrections.write() = updated.clone();
    Ok(CorrectionPruneResult {
        rules: updated,
        removed_unused: removed_unused_count,
        removed_orphaned_profiles: removed_orphaned_count,
        removed_stale: removed_stale_count,
    })
}

/// Remove learned correction rules for `heard` (and optionally specific `corrected`).
#[tauri::command]
pub async fn delete_learned_correction(
    state: State<'_, AppState>,
    heard: String,
    corrected: Option<String>,
    mode_affinity: Option<String>,
    app_profile_affinity: Option<String>,
) -> Result<Vec<LearnedCorrection>, String> {
    let heard = heard.trim().to_ascii_lowercase();
    if heard.is_empty() {
        return Err("'heard' is required.".into());
    }
    let corrected = corrected
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty());
    let mode_affinity = mode_affinity
        .as_ref()
        .map(|value| crate::settings::normalize_dictation_mode(value))
        .filter(|value| !value.is_empty());
    let app_profile_affinity = app_profile_affinity
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut settings = state.settings.lock();
    settings.learned_corrections.retain(|c| {
        !correction_matches_delete_target(
            c,
            &heard,
            corrected.as_deref(),
            mode_affinity.as_deref(),
            app_profile_affinity.as_deref(),
        )
    });
    settings.normalize();
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;

    let updated = settings.learned_corrections.clone();
    drop(settings);
    *state.learned_corrections.write() = updated.clone();
    Ok(updated)
}

fn correction_matches_delete_target(
    correction: &LearnedCorrection,
    heard: &str,
    corrected: Option<&str>,
    mode_affinity: Option<&str>,
    app_profile_affinity: Option<&str>,
) -> bool {
    if !correction.heard.eq_ignore_ascii_case(heard) {
        return false;
    }
    if let Some(corrected) = corrected {
        if !correction.corrected.eq_ignore_ascii_case(corrected) {
            return false;
        }
    }
    let scoped_delete = mode_affinity.is_some() || app_profile_affinity.is_some();
    if let Some(mode_affinity) = mode_affinity {
        if correction.mode_affinity.as_deref() != Some(mode_affinity) {
            return false;
        }
    } else if scoped_delete && correction.mode_affinity.is_some() {
        return false;
    }
    if let Some(app_profile_affinity) = app_profile_affinity {
        if correction.app_profile_affinity.as_deref() != Some(app_profile_affinity) {
            return false;
        }
    } else if scoped_delete && correction.app_profile_affinity.is_some() {
        return false;
    }
    true
}

/// Return configured per-app dictation profiles.
#[tauri::command]
pub async fn get_app_profiles(state: State<'_, AppState>) -> Result<Vec<AppProfile>, String> {
    Ok(state.settings.lock().app_profiles.clone())
}

/// Upsert a per-app dictation profile.
#[tauri::command]
pub async fn upsert_app_profile(
    state: State<'_, AppState>,
    profile: AppProfile,
) -> Result<Vec<AppProfile>, String> {
    let mut settings = state.settings.lock();
    if let Some(existing) = settings.app_profiles.iter_mut().find(|p| p.id == profile.id) {
        *existing = profile;
    } else {
        settings.app_profiles.push(profile);
    }
    settings.normalize();
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(settings.app_profiles.clone())
}

/// Delete a per-app dictation profile by id.
#[tauri::command]
pub async fn delete_app_profile(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<AppProfile>, String> {
    let mut settings = state.settings.lock();
    settings.app_profiles.retain(|profile| profile.id != id);
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(settings.app_profiles.clone())
}

/// Return the current foreground app and any matched app profile override.
#[tauri::command]
pub async fn get_active_app_context(state: State<'_, AppState>) -> Result<ActiveAppContext, String> {
    let settings = state.settings.lock();
    Ok(current_active_app_context(&settings))
}

fn current_active_app_context(settings: &crate::settings::AppSettings) -> ActiveAppContext {
    let foreground_app = text_injector::foreground_process_name();
    let matched_profile = resolve_app_profile(settings, foreground_app.as_deref());
    ActiveAppContext {
        foreground_app,
        matched_profile_id: matched_profile.map(|profile| profile.id.clone()),
        matched_profile_name: matched_profile.map(|profile| profile.name.clone()),
        dictation_mode: matched_profile
            .map(|profile| profile.dictation_mode.clone())
            .unwrap_or_else(|| settings.dictation_mode.clone()),
        phrase_bias_term_count: matched_profile
            .map(|profile| profile.phrase_bias_terms.len())
            .unwrap_or_else(|| settings.phrase_bias_terms.len()),
        post_utterance_refine: matched_profile
            .map(|profile| profile.post_utterance_refine)
            .unwrap_or(settings.post_utterance_refine),
    }
}

fn rule_has_valid_profile(
    settings: &crate::settings::AppSettings,
    rule: &LearnedCorrection,
) -> bool {
    match rule.app_profile_affinity.as_deref() {
        Some(profile_id) => settings.app_profiles.iter().any(|profile| profile.id == profile_id),
        None => true,
    }
}

fn rule_is_unused(rule: &LearnedCorrection) -> bool {
    rule.hits <= 1 && rule.last_used_at.is_none()
}

fn rule_is_stale(rule: &LearnedCorrection) -> bool {
    if rule.hits > 2 {
        return false;
    }
    let Some(last_used_at) = rule.last_used_at.as_deref() else {
        return false;
    };
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(last_used_at) else {
        return false;
    };
    let age = chrono::Utc::now().signed_duration_since(parsed.with_timezone(&chrono::Utc));
    age.num_days() >= 90
}

fn build_correction_diagnostics(settings: &crate::settings::AppSettings) -> CorrectionDiagnostics {
    let summarize_rule = |rule: &LearnedCorrection| CorrectionRuleSummary {
        heard: rule.heard.clone(),
        corrected: rule.corrected.clone(),
        hits: rule.hits,
        mode_affinity: rule.mode_affinity.clone(),
        app_profile_affinity: rule.app_profile_affinity.clone(),
        app_profile_name: rule.app_profile_affinity.as_ref().and_then(|id| {
            settings
                .app_profiles
                .iter()
                .find(|profile| profile.id == *id)
                .map(|profile| profile.name.clone())
        }),
        last_used_at: rule.last_used_at.clone(),
    };
    let mut top_rule_refs = settings.learned_corrections.iter().collect::<Vec<_>>();
    top_rule_refs.sort_by(|a, b| {
        b.hits
            .cmp(&a.hits)
            .then_with(|| b.last_used_at.cmp(&a.last_used_at))
            .then_with(|| a.heard.cmp(&b.heard))
    });
    let top_rules = top_rule_refs
        .into_iter()
        .take(12)
        .map(summarize_rule)
        .collect::<Vec<_>>();
    let mut recent_rule_refs = settings
        .learned_corrections
        .iter()
        .filter(|rule| rule.last_used_at.is_some())
        .collect::<Vec<_>>();
    recent_rule_refs.sort_by(|a, b| {
        b.last_used_at
            .cmp(&a.last_used_at)
            .then_with(|| b.hits.cmp(&a.hits))
            .then_with(|| a.heard.cmp(&b.heard))
    });
    let recent_rules = recent_rule_refs
        .into_iter()
        .take(8)
        .map(summarize_rule)
        .collect::<Vec<_>>();

    CorrectionDiagnostics {
        total_rules: settings.learned_corrections.len(),
        global_rules: settings
            .learned_corrections
            .iter()
            .filter(|rule| rule.mode_affinity.is_none() && rule.app_profile_affinity.is_none())
            .count(),
        mode_scoped_rules: settings
            .learned_corrections
            .iter()
            .filter(|rule| rule.mode_affinity.is_some() && rule.app_profile_affinity.is_none())
            .count(),
        profile_scoped_rules: settings
            .learned_corrections
            .iter()
            .filter(|rule| rule.app_profile_affinity.is_some())
            .count(),
        unused_rules: settings
            .learned_corrections
            .iter()
            .filter(|rule| rule_is_unused(rule))
            .count(),
        orphaned_profile_rules: settings
            .learned_corrections
            .iter()
            .filter(|rule| !rule_has_valid_profile(settings, rule))
            .count(),
        stale_rules: settings
            .learned_corrections
            .iter()
            .filter(|rule| rule_is_stale(rule))
            .count(),
        top_rules,
        recent_rules,
    }
}

/// Persist runtime settings.
///
/// These settings are applied immediately for env-backed toggles and on next app
/// start for model/runtime reload.
#[tauri::command]
pub async fn set_runtime_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    model_profile: Option<String>,
    performance_profile: Option<String>,
    dictation_mode: Option<String>,
    toggle_shortcut: Option<String>,
    ort_ep: Option<String>,
    ort_intra_threads: Option<usize>,
    ort_inter_threads: Option<usize>,
    ort_parallel: Option<bool>,
    language_hint: Option<String>,
    pill_visualizer_sensitivity: Option<f32>,
    activity_sensitivity: Option<f32>,
    activity_noise_gate: Option<f32>,
    activity_clip_threshold: Option<f32>,
    input_gain_boost: Option<f32>,
    post_utterance_refine: Option<bool>,
    phrase_bias_terms: Option<Vec<String>>,
    openai_api_key: Option<String>,
    cloud_mode: Option<String>,
    cloud_opt_in: Option<bool>,
    reliability_mode: Option<bool>,
    onboarding_completed: Option<bool>,
    history_enabled: Option<bool>,
    retention_days: Option<usize>,
) -> Result<RuntimeSettings, String> {
    let mut settings = state.settings.lock();
    let previous_shortcut = settings.toggle_shortcut.clone();

    if let Some(profile) = model_profile {
        settings.model_profile = normalize_model_profile(&profile);
    }
    if let Some(profile) = performance_profile {
        settings.performance_profile = normalize_performance_profile(&profile);
    }
    if let Some(mode) = dictation_mode {
        settings.dictation_mode = normalize_dictation_mode(&mode);
    }
    if let Some(shortcut) = toggle_shortcut {
        settings.toggle_shortcut = normalize_toggle_shortcut(&shortcut);
    }
    if let Some(ep) = ort_ep {
        settings.ort_ep = normalize_ort_ep(&ep);
    }
    if let Some(v) = ort_intra_threads {
        settings.ort_intra_threads = v.clamp(0, 32);
    }
    if let Some(v) = ort_inter_threads {
        settings.ort_inter_threads = v.clamp(0, 8);
    }
    if let Some(v) = ort_parallel {
        settings.ort_parallel = v;
    }
    if let Some(hint) = language_hint {
        settings.language_hint = normalize_language_hint(&hint);
    }
    if let Some(v) = pill_visualizer_sensitivity {
        settings.pill_visualizer_sensitivity = v;
    }
    if let Some(v) = activity_sensitivity {
        settings.activity_sensitivity = v;
    }
    if let Some(v) = activity_noise_gate {
        settings.activity_noise_gate = v;
    }
    if let Some(v) = activity_clip_threshold {
        settings.activity_clip_threshold = v;
    }
    if let Some(v) = input_gain_boost {
        settings.input_gain_boost = v;
    }
    if let Some(v) = post_utterance_refine {
        settings.post_utterance_refine = v;
    }
    if let Some(v) = phrase_bias_terms {
        settings.phrase_bias_terms = v;
    }
    if let Some(v) = openai_api_key {
        settings.openai_api_key = Some(v);
    }
    if let Some(v) = cloud_mode.as_ref() {
        settings.cloud_mode = normalize_cloud_mode(v);
    }
    if let Some(v) = cloud_opt_in {
        settings.cloud_mode = if v {
            if settings.cloud_mode == "cloud_preferred" {
                "cloud_preferred".into()
            } else {
                "hybrid".into()
            }
        } else {
            "local_only".into()
        };
        settings.cloud_opt_in = settings.cloud_mode != "local_only";
    }
    if let Some(v) = reliability_mode {
        settings.reliability_mode = v;
    }
    if let Some(v) = onboarding_completed {
        settings.onboarding_completed = v;
    }
    if let Some(v) = history_enabled {
        settings.history_enabled = v;
    }
    if let Some(v) = retention_days {
        settings.retention_days = v.clamp(1, 3650);
    }
    settings.normalize();

    let global_shortcut = app.global_shortcut();
    if settings.toggle_shortcut != previous_shortcut {
        if global_shortcut.is_registered(previous_shortcut.as_str()) {
            global_shortcut
                .unregister(previous_shortcut.as_str())
                .map_err(|e| format!("failed to unregister previous shortcut: {e}"))?;
        }
        if let Err(e) = global_shortcut.register(settings.toggle_shortcut.as_str()) {
            // Attempt best-effort rollback so the app keeps a working keybinding.
            let _ = global_shortcut.register(previous_shortcut.as_str());
            return Err(format!(
                "failed to register shortcut '{}': {e}",
                settings.toggle_shortcut
            ));
        }
    }

    sync_runtime_with_settings(state.engine.as_ref(), &settings, RuntimeEnvMode::Overwrite);
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    state.store.prune_history(settings.retention_days)?;
    Ok(settings.runtime_settings())
}

#[tauri::command]
pub async fn get_perf_snapshot(state: State<'_, AppState>) -> Result<PerfSnapshot, String> {
    Ok(state.perf_snapshot())
}

#[tauri::command]
pub async fn get_diagnostics_bundle(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<DiagnosticsBundle, String> {
    let settings = state.settings.lock();
    let active_app_context = current_active_app_context(&settings);
    let runtime_settings = settings.runtime_settings();
    let privacy_settings = PrivacySettings {
        history_enabled: runtime_settings.history_enabled,
        retention_days: runtime_settings.retention_days,
        cloud_opt_in: runtime_settings.cloud_opt_in,
    };
    let correction_diagnostics = build_correction_diagnostics(&settings);
    let update_repo_slug =
        normalize_repo_slug(None).unwrap_or_else(|_| DEFAULT_UPDATE_REPO_SLUG.to_string());
    drop(settings);

    Ok(DiagnosticsBundle {
        generated_at: chrono::Utc::now().to_rfc3339(),
        app_version: app.package_info().version.to_string(),
        update_repo_slug,
        settings_path: state.settings_path.display().to_string(),
        active_app_context,
        runtime_settings,
        privacy_settings,
        perf_snapshot: state.perf_snapshot(),
        history_storage: state.store.history_storage_summary()?,
        devices: dictum_core::audio::device::list_input_devices(),
        correction_diagnostics,
    })
}

#[tauri::command]
pub async fn export_diagnostics_bundle(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<DiagnosticsExportResult, String> {
    let bundle = get_diagnostics_bundle(app, state).await?;
    let settings_path = std::path::PathBuf::from(&bundle.settings_path);
    let export_dir = settings_path
        .parent()
        .map(|parent| parent.join("diagnostics"))
        .unwrap_or_else(|| std::path::PathBuf::from("diagnostics"));
    fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file_name = format!("dictum-diagnostics-{stamp}.json");
    let path = export_dir.join(&file_name);
    let payload = serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())?;
    fs::write(&path, payload).map_err(|e| e.to_string())?;
    Ok(DiagnosticsExportResult {
        path: path.display().to_string(),
        file_name,
    })
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
        settings.cloud_mode = if v {
            if settings.cloud_mode == "cloud_preferred" {
                "cloud_preferred".into()
            } else {
                "hybrid".into()
            }
        } else {
            "local_only".into()
        };
        settings.cloud_opt_in = settings.cloud_mode != "local_only";
    }
    settings.normalize();
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    state.store.prune_history(settings.retention_days)?;
    sync_runtime_with_settings(state.engine.as_ref(), &settings, RuntimeEnvMode::Overwrite);
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

#[cfg(test)]
mod tests {
    use super::{
        build_correction_diagnostics, correction_matches_delete_target, normalize_repo_slug, parse_sha256_from_sums,
        select_checksums_asset, select_installer_asset, version_tuple, GitHubAsset,
    };
    use crate::settings::{AppProfile, AppSettings, LearnedCorrection};

    fn asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.to_string(),
            browser_download_url: format!("https://example.invalid/{name}"),
        }
    }

    #[test]
    fn normalize_repo_slug_maps_legacy_slug() {
        let slug = normalize_repo_slug(Some("LatticeLabs/Dictum".into())).expect("slug");
        assert_eq!(slug, "sinergaoptima/dictum");
    }

    #[test]
    fn normalize_repo_slug_rejects_invalid_shape() {
        assert!(normalize_repo_slug(Some("invalid".into())).is_err());
        assert!(normalize_repo_slug(Some("owner/repo/extra".into())).is_err());
        assert!(normalize_repo_slug(Some("bad!/repo".into())).is_err());
    }

    #[test]
    fn version_tuple_parses_semver_and_strips_prefixes() {
        assert_eq!(version_tuple("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(version_tuple("1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(version_tuple("V2.0"), Some((2, 0, 0)));
    }

    #[test]
    fn select_installer_asset_prefers_setup_exe() {
        let assets = vec![
            asset("Dictum_0.1.7_x64.msi"),
            asset("Dictum_0.1.7_x64-setup.exe"),
            asset("dictum.exe"),
        ];
        let picked = select_installer_asset(&assets).expect("installer");
        assert_eq!(picked.name, "Dictum_0.1.7_x64-setup.exe");
    }

    #[test]
    fn select_checksums_asset_finds_manifest_case_insensitively() {
        let assets = vec![asset("notes.txt"), asset("SHA256SUMS.txt")];
        let picked = select_checksums_asset(&assets).expect("checksums");
        assert_eq!(picked.name, "SHA256SUMS.txt");
    }

    #[test]
    fn parse_sha256_from_sums_supports_common_formats() {
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let contents = format!("{hash}  Dictum_0.1.7_x64-setup.exe\n");
        let parsed = parse_sha256_from_sums(&contents, "Dictum_0.1.7_x64-setup.exe");
        assert_eq!(parsed.as_deref(), Some(hash));

        let reverse = format!("Dictum_0.1.7_x64-setup.exe {hash}\n");
        let parsed_reverse = parse_sha256_from_sums(&reverse, "Dictum_0.1.7_x64-setup.exe");
        assert_eq!(parsed_reverse.as_deref(), Some(hash));
    }

    #[test]
    fn correction_delete_target_matches_exact_scoped_rule() {
        let rule = LearnedCorrection {
            heard: "printf".into(),
            corrected: "println!".into(),
            hits: 4,
            mode_affinity: Some("coding".into()),
            app_profile_affinity: Some("cursor".into()),
            last_used_at: Some("2026-03-07T10:00:00Z".into()),
        };

        assert!(correction_matches_delete_target(
            &rule,
            "printf",
            Some("println!"),
            Some("coding"),
            Some("cursor"),
        ));
        assert!(!correction_matches_delete_target(
            &rule,
            "printf",
            Some("println!"),
            Some("coding"),
            None,
        ));
        assert!(!correction_matches_delete_target(
            &rule,
            "printf",
            Some("println!"),
            None,
            Some("cursor"),
        ));
    }

    #[test]
    fn correction_delete_target_preserves_unscoped_delete_behavior() {
        let global = LearnedCorrection {
            heard: "ladder labs".into(),
            corrected: "Lattice Labs".into(),
            hits: 1,
            mode_affinity: None,
            app_profile_affinity: None,
            last_used_at: None,
        };
        let scoped = LearnedCorrection {
            heard: "ladder labs".into(),
            corrected: "Lattice Labs".into(),
            hits: 3,
            mode_affinity: Some("conversation".into()),
            app_profile_affinity: None,
            last_used_at: Some("2026-03-07T11:00:00Z".into()),
        };

        assert!(correction_matches_delete_target(
            &global,
            "ladder labs",
            Some("lattice labs"),
            None,
            None,
        ));
        assert!(correction_matches_delete_target(
            &scoped,
            "ladder labs",
            Some("lattice labs"),
            None,
            None,
        ));
    }

    #[test]
    fn correction_diagnostics_count_orphaned_and_stale_rules() {
        let mut settings = AppSettings::default();
        settings.app_profiles = vec![AppProfile {
            id: "cursor-profile".into(),
            name: "Cursor".into(),
            app_match: "cursor.exe".into(),
            dictation_mode: "coding".into(),
            phrase_bias_terms: Vec::new(),
            post_utterance_refine: false,
            enabled: true,
        }];
        settings.learned_corrections = vec![
            LearnedCorrection {
                heard: "printf".into(),
                corrected: "println!".into(),
                hits: 1,
                mode_affinity: Some("coding".into()),
                app_profile_affinity: Some("missing-profile".into()),
                last_used_at: None,
            },
            LearnedCorrection {
                heard: "ship it".into(),
                corrected: "ShipIt".into(),
                hits: 2,
                mode_affinity: None,
                app_profile_affinity: None,
                last_used_at: Some("2025-10-01T10:00:00Z".into()),
            },
        ];
        settings.normalize();

        let diagnostics = build_correction_diagnostics(&settings);
        assert_eq!(diagnostics.total_rules, 2);
        assert_eq!(diagnostics.unused_rules, 1);
        assert_eq!(diagnostics.orphaned_profile_rules, 1);
        assert_eq!(diagnostics.stale_rules, 1);
    }
}
