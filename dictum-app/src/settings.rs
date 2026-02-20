//! Persistent application settings (JSON file in app data directory).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AppSettings {
    pub preferred_input_device: Option<String>,
    pub model_profile: String,
    pub ort_ep: String,
    pub language_hint: String,
    pub cloud_opt_in: bool,
    pub history_enabled: bool,
    pub retention_days: usize,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            preferred_input_device: None,
            model_profile: "large-v3-turbo".into(),
            ort_ep: "auto".into(),
            language_hint: "auto".into(),
            cloud_opt_in: false,
            history_enabled: true,
            retention_days: 90,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSettings {
    pub model_profile: String,
    pub ort_ep: String,
    pub language_hint: String,
    pub cloud_opt_in: bool,
    pub history_enabled: bool,
    pub retention_days: usize,
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.model_profile = normalize_model_profile(&self.model_profile);
        self.ort_ep = normalize_ort_ep(&self.ort_ep);
        self.language_hint = normalize_language_hint(&self.language_hint);
        self.retention_days = self.retention_days.clamp(1, 3650);
        self.preferred_input_device = self
            .preferred_input_device
            .as_ref()
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty());
    }

    pub fn runtime_settings(&self) -> RuntimeSettings {
        RuntimeSettings {
            model_profile: self.model_profile.clone(),
            ort_ep: self.ort_ep.clone(),
            language_hint: self.language_hint.clone(),
            cloud_opt_in: self.cloud_opt_in,
            history_enabled: self.history_enabled,
            retention_days: self.retention_days,
        }
    }
}

pub fn normalize_model_profile(raw: &str) -> String {
    let profile = raw.trim().to_ascii_lowercase();
    if profile.is_empty() {
        "large-v3-turbo".into()
    } else {
        profile
    }
}

pub fn normalize_ort_ep(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "cpu" => "cpu".into(),
        "dml" | "directml" => "directml".into(),
        _ => "auto".into(),
    }
}

pub fn normalize_language_hint(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "en" | "eng" | "english" => "english".into(),
        "zh" | "zh-cn" | "zh-hans" | "mandarin" | "chinese" => "mandarin".into(),
        "ru" | "rus" | "russian" => "russian".into(),
        _ => "auto".into(),
    }
}

pub fn apply_runtime_env_from_settings(settings: &AppSettings) {
    if std::env::var("DICTUM_MODEL_PROFILE").is_err() {
        if settings.model_profile.eq_ignore_ascii_case("small") {
            std::env::remove_var("DICTUM_MODEL_PROFILE");
        } else {
            std::env::set_var("DICTUM_MODEL_PROFILE", &settings.model_profile);
        }
    }

    if std::env::var("DICTUM_ORT_EP").is_err() {
        std::env::set_var("DICTUM_ORT_EP", &settings.ort_ep);
    }
    if std::env::var("DICTUM_LANGUAGE_HINT").is_err() {
        std::env::set_var("DICTUM_LANGUAGE_HINT", &settings.language_hint);
    }
    if std::env::var("DICTUM_CLOUD_FALLBACK").is_err() {
        std::env::set_var(
            "DICTUM_CLOUD_FALLBACK",
            if settings.cloud_opt_in { "1" } else { "0" },
        );
    }
}

pub fn default_settings_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Lattice Labs")
            .join("Dictum")
            .join("settings.json")
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(".local")
                    .join("share")
            })
            .join("dictum")
            .join("settings.json")
    }
}

pub fn load_settings(path: &Path) -> AppSettings {
    let mut settings = fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<AppSettings>(&raw).ok())
        .unwrap_or_default();
    settings.normalize();
    settings
}

pub fn save_settings(path: &Path, settings: &AppSettings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    fs::write(path, json)
}
