//! Tauri command handlers.
//!
//! Each function is registered with `tauri::Builder::invoke_handler` and
//! callable from the frontend via `invoke(...)`.

use dictum_core::{audio::device::DeviceInfo, ipc::events::EngineStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tracing::{info, warn};

use crate::settings::{
    normalize_cloud_mode, normalize_language_hint, normalize_model_profile, normalize_ort_ep,
    normalize_performance_profile, normalize_toggle_shortcut, save_settings, LearnedCorrection,
    RuntimeSettings,
};
use crate::model_profiles::{
    model_profile_catalog, recommend_model_profile, ModelProfileMetadata,
    ModelProfileRecommendation,
};
use crate::state::{AppState, PerfSnapshot};
use crate::storage::{DictionaryEntry, HistoryPage, PrivacySettings, SnippetEntry, StatsPayload};

const DEFAULT_UPDATE_REPO_SLUG: &str = "latticelabs/dictum";
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
    let slug = chosen.trim().trim_matches('/').to_ascii_lowercase();
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

        let name_normalized = name_part.trim().trim_start_matches('*').to_ascii_lowercase();
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
        Err(format!("installer signature status is '{status}', expected 'Valid'"))
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
    let has_update = match (version_tuple(&current_version), version_tuple(&latest_version)) {
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
    let install_path =
        tauri::async_runtime::spawn_blocking(move || -> Result<(std::path::PathBuf, String), String> {
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
        })
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

    std::env::set_var("DICTUM_MODEL_PROFILE", settings.model_profile.clone());
    std::env::set_var("DICTUM_ORT_EP", settings.ort_ep.clone());
    std::env::set_var(
        "DICTUM_PERFORMANCE_PROFILE",
        settings.performance_profile.clone(),
    );
    std::env::set_var("DICTUM_ORT_INTRA_THREADS", settings.ort_intra_threads.to_string());
    std::env::set_var("DICTUM_ORT_INTER_THREADS", settings.ort_inter_threads.to_string());
    std::env::set_var(
        "DICTUM_ORT_PARALLEL",
        if settings.ort_parallel { "1" } else { "0" },
    );

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
    let clip_threshold = (normal_p80.max(ambient_p90 * 12.0).max(whisper_p70 * 8.0)).clamp(0.12, 0.95);

    settings.activity_noise_gate = noise_gate;
    settings.activity_sensitivity = activity_sensitivity;
    settings.pill_visualizer_sensitivity = pill_sensitivity;
    settings.input_gain_boost = input_gain_boost;
    settings.activity_clip_threshold = clip_threshold;
    settings.normalize();

    std::env::set_var("DICTUM_MODEL_PROFILE", settings.model_profile.clone());
    std::env::set_var("DICTUM_ORT_EP", settings.ort_ep.clone());
    std::env::set_var(
        "DICTUM_PERFORMANCE_PROFILE",
        settings.performance_profile.clone(),
    );
    std::env::set_var("DICTUM_ORT_INTRA_THREADS", settings.ort_intra_threads.to_string());
    std::env::set_var("DICTUM_ORT_INTER_THREADS", settings.ort_inter_threads.to_string());
    std::env::set_var(
        "DICTUM_ORT_PARALLEL",
        if settings.ort_parallel { "1" } else { "0" },
    );
    std::env::set_var(
        "DICTUM_INPUT_GAIN_BOOST",
        format!("{:.4}", settings.input_gain_boost),
    );

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
) -> Result<Vec<LearnedCorrection>, String> {
    let heard = heard.trim().to_ascii_lowercase();
    let corrected = corrected.trim().to_string();
    if heard.is_empty() || corrected.is_empty() {
        return Err("Both 'heard' and 'corrected' are required.".into());
    }

    let mut settings = state.settings.lock();
    if let Some(existing) = settings
        .learned_corrections
        .iter_mut()
        .find(|c| c.heard.eq_ignore_ascii_case(&heard))
    {
        existing.corrected = corrected.clone();
        existing.hits = existing.hits.saturating_add(1);
    } else {
        settings.learned_corrections.push(LearnedCorrection {
            heard: heard.clone(),
            corrected: corrected.clone(),
            hits: 1,
        });
    }
    settings
        .learned_corrections
        .sort_by(|a, b| b.hits.cmp(&a.hits).then_with(|| a.heard.cmp(&b.heard)));
    settings.normalize();
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;

    let updated = settings.learned_corrections.clone();
    drop(settings);
    *state.learned_corrections.write() = updated.clone();
    Ok(updated)
}

/// Remove learned correction rules for `heard` (and optionally specific `corrected`).
#[tauri::command]
pub async fn delete_learned_correction(
    state: State<'_, AppState>,
    heard: String,
    corrected: Option<String>,
) -> Result<Vec<LearnedCorrection>, String> {
    let heard = heard.trim().to_ascii_lowercase();
    if heard.is_empty() {
        return Err("'heard' is required.".into());
    }
    let corrected = corrected
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty());

    let mut settings = state.settings.lock();
    settings.learned_corrections.retain(|c| {
        if !c.heard.eq_ignore_ascii_case(&heard) {
            return true;
        }
        if let Some(ref corr) = corrected {
            !c.corrected.eq_ignore_ascii_case(corr)
        } else {
            false
        }
    });
    settings.normalize();
    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;

    let updated = settings.learned_corrections.clone();
    drop(settings);
    *state.learned_corrections.write() = updated.clone();
    Ok(updated)
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

    std::env::set_var("DICTUM_MODEL_PROFILE", settings.model_profile.clone());
    std::env::set_var("DICTUM_ORT_EP", settings.ort_ep.clone());
    if settings.ort_intra_threads > 0 {
        std::env::set_var("DICTUM_ORT_INTRA_THREADS", settings.ort_intra_threads.to_string());
    } else {
        std::env::remove_var("DICTUM_ORT_INTRA_THREADS");
    }
    if settings.ort_inter_threads > 0 {
        std::env::set_var("DICTUM_ORT_INTER_THREADS", settings.ort_inter_threads.to_string());
    } else {
        std::env::remove_var("DICTUM_ORT_INTER_THREADS");
    }
    std::env::set_var(
        "DICTUM_ORT_PARALLEL",
        if settings.ort_parallel { "1" } else { "0" },
    );
    std::env::set_var("DICTUM_LANGUAGE_HINT", settings.language_hint.clone());
    std::env::set_var(
        "DICTUM_PERFORMANCE_PROFILE",
        settings.performance_profile.clone(),
    );
    std::env::set_var(
        "DICTUM_CLOUD_FALLBACK",
        if settings.cloud_mode == "local_only" {
            "0"
        } else {
            "1"
        },
    );
    std::env::set_var("DICTUM_CLOUD_MODE", settings.cloud_mode.clone());
    std::env::set_var(
        "DICTUM_INPUT_GAIN_BOOST",
        format!("{:.4}", settings.input_gain_boost),
    );
    std::env::set_var(
        "DICTUM_POST_UTTERANCE_REFINEMENT",
        if settings.post_utterance_refine {
            "1"
        } else {
            "0"
        },
    );
    std::env::set_var(
        "DICTUM_PHRASE_BIAS_TERMS",
        settings.phrase_bias_terms.join("\n"),
    );
    std::env::set_var(
        "DICTUM_RELIABILITY_MODE",
        if settings.reliability_mode { "1" } else { "0" },
    );
    if let Some(api_key) = settings.openai_api_key.as_ref() {
        std::env::set_var("DICTUM_OPENAI_API_KEY", api_key);
    } else {
        std::env::remove_var("DICTUM_OPENAI_API_KEY");
    }

    save_settings(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    state.store.prune_history(settings.retention_days)?;
    Ok(settings.runtime_settings())
}

#[tauri::command]
pub async fn get_perf_snapshot(state: State<'_, AppState>) -> Result<PerfSnapshot, String> {
    Ok(state.perf_snapshot())
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
    std::env::set_var(
        "DICTUM_CLOUD_FALLBACK",
        if settings.cloud_mode == "local_only" {
            "0"
        } else {
            "1"
        },
    );
    std::env::set_var("DICTUM_CLOUD_MODE", settings.cloud_mode.clone());
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
