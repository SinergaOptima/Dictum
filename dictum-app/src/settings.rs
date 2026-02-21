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
    pub performance_profile: String,
    pub toggle_shortcut: String,
    pub ort_ep: String,
    pub language_hint: String,
    pub pill_visualizer_sensitivity: f32,
    pub activity_sensitivity: f32,
    pub activity_noise_gate: f32,
    pub activity_clip_threshold: f32,
    pub input_gain_boost: f32,
    pub post_utterance_refine: bool,
    pub phrase_bias_terms: Vec<String>,
    pub openai_api_key: Option<String>,
    pub cloud_opt_in: bool,
    pub history_enabled: bool,
    pub retention_days: usize,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            preferred_input_device: None,
            model_profile: "large-v3-turbo".into(),
            performance_profile: "whisper_balanced_english".into(),
            toggle_shortcut: "Ctrl+Shift+Space".into(),
            ort_ep: "auto".into(),
            language_hint: "english".into(),
            pill_visualizer_sensitivity: 10.0,
            activity_sensitivity: 4.2,
            activity_noise_gate: 0.0015,
            activity_clip_threshold: 0.32,
            input_gain_boost: 1.0,
            post_utterance_refine: false,
            phrase_bias_terms: Vec::new(),
            openai_api_key: None,
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
    pub performance_profile: String,
    pub toggle_shortcut: String,
    pub ort_ep: String,
    pub language_hint: String,
    pub pill_visualizer_sensitivity: f32,
    pub activity_sensitivity: f32,
    pub activity_noise_gate: f32,
    pub activity_clip_threshold: f32,
    pub input_gain_boost: f32,
    pub post_utterance_refine: bool,
    pub phrase_bias_terms: Vec<String>,
    pub has_openai_api_key: bool,
    pub cloud_opt_in: bool,
    pub history_enabled: bool,
    pub retention_days: usize,
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.model_profile = normalize_model_profile(&self.model_profile);
        self.performance_profile = normalize_performance_profile(&self.performance_profile);
        self.toggle_shortcut = normalize_toggle_shortcut(&self.toggle_shortcut);
        self.ort_ep = normalize_ort_ep(&self.ort_ep);
        self.language_hint = normalize_language_hint(&self.language_hint);
        self.pill_visualizer_sensitivity = self.pill_visualizer_sensitivity.clamp(1.0, 20.0);
        self.activity_sensitivity = self.activity_sensitivity.clamp(1.0, 20.0);
        self.activity_noise_gate = self.activity_noise_gate.clamp(0.0, 0.1);
        self.activity_clip_threshold = self.activity_clip_threshold.clamp(0.02, 1.0);
        self.input_gain_boost = self.input_gain_boost.clamp(0.5, 8.0);
        self.phrase_bias_terms = normalize_phrase_bias_terms(&self.phrase_bias_terms);
        self.openai_api_key = self
            .openai_api_key
            .as_ref()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        if self.performance_profile == "whisper_balanced_english" {
            self.language_hint = "english".into();
        }
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
            performance_profile: self.performance_profile.clone(),
            toggle_shortcut: self.toggle_shortcut.clone(),
            ort_ep: self.ort_ep.clone(),
            language_hint: self.language_hint.clone(),
            pill_visualizer_sensitivity: self.pill_visualizer_sensitivity,
            activity_sensitivity: self.activity_sensitivity,
            activity_noise_gate: self.activity_noise_gate,
            activity_clip_threshold: self.activity_clip_threshold,
            input_gain_boost: self.input_gain_boost,
            post_utterance_refine: self.post_utterance_refine,
            phrase_bias_terms: self.phrase_bias_terms.clone(),
            has_openai_api_key: self.openai_api_key.is_some(),
            cloud_opt_in: self.cloud_opt_in,
            history_enabled: self.history_enabled,
            retention_days: self.retention_days,
        }
    }
}

pub fn normalize_model_profile(raw: &str) -> String {
    let profile = raw.trim().to_ascii_lowercase();
    match profile.as_str() {
        "" => "large-v3-turbo".into(),
        "turbo" | "whisper-large-v3-turbo" => "large-v3-turbo".into(),
        "large" | "whisper-large-v3" => "large-v3".into(),
        "distil" | "distil-whisper-large-v3" => "distil-large-v3".into(),
        "medium-en" => "medium.en".into(),
        "small-en" => "small.en".into(),
        "base-en" => "base.en".into(),
        "tiny-en" => "tiny.en".into(),
        _ => profile,
    }
}

pub fn normalize_performance_profile(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "whisper" | "whisper_balanced_english" | "whisper_english" => {
            "whisper_balanced_english".into()
        }
        "stability" | "long_form" | "stability_long_form" => "stability_long_form".into(),
        "balanced" | "balanced_general" => "balanced_general".into(),
        "latency" | "short_utterance" | "latency_short_utterance" => {
            "latency_short_utterance".into()
        }
        _ => "whisper_balanced_english".into(),
    }
}

pub fn normalize_toggle_shortcut(raw: &str) -> String {
    let normalized = raw.trim();
    if normalized.is_empty() {
        "Ctrl+Shift+Space".into()
    } else {
        normalized.into()
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

fn normalize_phrase_bias_terms(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for term in raw {
        let normalized = term.trim();
        if normalized.is_empty() {
            continue;
        }
        if out.iter().any(|t: &String| t.eq_ignore_ascii_case(normalized)) {
            continue;
        }
        out.push(normalized.to_string());
        if out.len() >= 64 {
            break;
        }
    }
    out
}

pub fn apply_runtime_env_from_settings(settings: &AppSettings) {
    if std::env::var("DICTUM_MODEL_PROFILE").is_err() {
        std::env::set_var("DICTUM_MODEL_PROFILE", &settings.model_profile);
    }

    if std::env::var("DICTUM_ORT_EP").is_err() {
        std::env::set_var("DICTUM_ORT_EP", &settings.ort_ep);
    }
    if std::env::var("DICTUM_PERFORMANCE_PROFILE").is_err() {
        std::env::set_var("DICTUM_PERFORMANCE_PROFILE", &settings.performance_profile);
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
    if std::env::var("DICTUM_OPENAI_API_KEY").is_err() {
        if let Some(key) = settings.openai_api_key.as_ref() {
            std::env::set_var("DICTUM_OPENAI_API_KEY", key);
        }
    }
    if std::env::var("DICTUM_INPUT_GAIN_BOOST").is_err() {
        std::env::set_var(
            "DICTUM_INPUT_GAIN_BOOST",
            format!("{:.4}", settings.input_gain_boost),
        );
    }
    if std::env::var("DICTUM_POST_UTTERANCE_REFINEMENT").is_err() {
        std::env::set_var(
            "DICTUM_POST_UTTERANCE_REFINEMENT",
            if settings.post_utterance_refine {
                "1"
            } else {
                "0"
            },
        );
    }
    if std::env::var("DICTUM_PHRASE_BIAS_TERMS").is_err() {
        std::env::set_var("DICTUM_PHRASE_BIAS_TERMS", settings.phrase_bias_terms.join("\n"));
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
