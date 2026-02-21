//! Persistent application settings (JSON file in app data directory).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedCorrection {
    pub heard: String,
    pub corrected: String,
    pub hits: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AppSettings {
    pub preferred_input_device: Option<String>,
    pub model_profile: String,
    pub performance_profile: String,
    pub toggle_shortcut: String,
    pub ort_ep: String,
    pub ort_intra_threads: usize,
    pub ort_inter_threads: usize,
    pub ort_parallel: bool,
    pub language_hint: String,
    pub pill_visualizer_sensitivity: f32,
    pub activity_sensitivity: f32,
    pub activity_noise_gate: f32,
    pub activity_clip_threshold: f32,
    pub input_gain_boost: f32,
    pub post_utterance_refine: bool,
    pub phrase_bias_terms: Vec<String>,
    pub openai_api_key: Option<String>,
    pub cloud_mode: String,
    pub cloud_opt_in: bool,
    pub reliability_mode: bool,
    pub onboarding_completed: bool,
    pub history_enabled: bool,
    pub retention_days: usize,
    pub learned_corrections: Vec<LearnedCorrection>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            preferred_input_device: None,
            model_profile: "distil-large-v3".into(),
            performance_profile: "whisper_balanced_english".into(),
            toggle_shortcut: "Ctrl+Shift+Space".into(),
            ort_ep: "auto".into(),
            ort_intra_threads: 0,
            ort_inter_threads: 0,
            ort_parallel: true,
            language_hint: "english".into(),
            pill_visualizer_sensitivity: 10.0,
            activity_sensitivity: 4.2,
            activity_noise_gate: 0.0015,
            activity_clip_threshold: 0.32,
            input_gain_boost: 1.0,
            post_utterance_refine: false,
            phrase_bias_terms: Vec::new(),
            openai_api_key: None,
            cloud_mode: "local_only".into(),
            cloud_opt_in: false,
            reliability_mode: true,
            onboarding_completed: false,
            history_enabled: true,
            retention_days: 90,
            learned_corrections: Vec::new(),
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
    pub ort_intra_threads: usize,
    pub ort_inter_threads: usize,
    pub ort_parallel: bool,
    pub language_hint: String,
    pub pill_visualizer_sensitivity: f32,
    pub activity_sensitivity: f32,
    pub activity_noise_gate: f32,
    pub activity_clip_threshold: f32,
    pub input_gain_boost: f32,
    pub post_utterance_refine: bool,
    pub phrase_bias_terms: Vec<String>,
    pub has_openai_api_key: bool,
    pub cloud_mode: String,
    pub cloud_opt_in: bool,
    pub reliability_mode: bool,
    pub onboarding_completed: bool,
    pub history_enabled: bool,
    pub retention_days: usize,
    pub correction_count: usize,
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.model_profile = normalize_model_profile(&self.model_profile);
        self.performance_profile = normalize_performance_profile(&self.performance_profile);
        self.toggle_shortcut = normalize_toggle_shortcut(&self.toggle_shortcut);
        self.ort_ep = normalize_ort_ep(&self.ort_ep);
        self.ort_intra_threads = self.ort_intra_threads.clamp(0, 32);
        self.ort_inter_threads = self.ort_inter_threads.clamp(0, 8);
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
        let inferred_cloud_mode = if self.cloud_mode.trim().is_empty()
            || (self.cloud_mode.trim().eq_ignore_ascii_case("local_only") && self.cloud_opt_in)
        {
            if self.cloud_opt_in {
                "hybrid"
            } else {
                "local_only"
            }
        } else {
            self.cloud_mode.as_str()
        };
        self.cloud_mode = normalize_cloud_mode(inferred_cloud_mode);
        self.cloud_opt_in = self.cloud_mode != "local_only";
        if self.performance_profile == "whisper_balanced_english" {
            self.language_hint = "english".into();
        }
        self.retention_days = self.retention_days.clamp(1, 3650);
        self.learned_corrections = normalize_learned_corrections(&self.learned_corrections);
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
            ort_intra_threads: self.ort_intra_threads,
            ort_inter_threads: self.ort_inter_threads,
            ort_parallel: self.ort_parallel,
            language_hint: self.language_hint.clone(),
            pill_visualizer_sensitivity: self.pill_visualizer_sensitivity,
            activity_sensitivity: self.activity_sensitivity,
            activity_noise_gate: self.activity_noise_gate,
            activity_clip_threshold: self.activity_clip_threshold,
            input_gain_boost: self.input_gain_boost,
            post_utterance_refine: self.post_utterance_refine,
            phrase_bias_terms: self.phrase_bias_terms.clone(),
            has_openai_api_key: self.openai_api_key.is_some(),
            cloud_mode: self.cloud_mode.clone(),
            cloud_opt_in: self.cloud_opt_in,
            reliability_mode: self.reliability_mode,
            onboarding_completed: self.onboarding_completed,
            history_enabled: self.history_enabled,
            retention_days: self.retention_days,
            correction_count: self.learned_corrections.len(),
        }
    }
}

pub fn normalize_model_profile(raw: &str) -> String {
    let profile = raw.trim().to_ascii_lowercase();
    match profile.as_str() {
        "" => "distil-large-v3".into(),
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

pub fn normalize_cloud_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "cloud" | "prefer_cloud" | "cloud_preferred" => "cloud_preferred".into(),
        "hybrid" | "fallback" | "cloud_fallback" => "hybrid".into(),
        _ => "local_only".into(),
    }
}

fn normalize_phrase_bias_terms(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for term in raw {
        let normalized = term.trim();
        if normalized.is_empty() {
            continue;
        }
        if out
            .iter()
            .any(|t: &String| t.eq_ignore_ascii_case(normalized))
        {
            continue;
        }
        out.push(normalized.to_string());
        if out.len() >= 64 {
            break;
        }
    }
    out
}

fn normalize_learned_corrections(raw: &[LearnedCorrection]) -> Vec<LearnedCorrection> {
    let mut out = Vec::new();
    for item in raw {
        let heard = item.heard.trim().to_ascii_lowercase();
        let corrected = item.corrected.trim().to_string();
        if heard.is_empty() || corrected.is_empty() {
            continue;
        }
        if out.iter().any(|e: &LearnedCorrection| {
            e.heard.eq_ignore_ascii_case(&heard) && e.corrected.eq_ignore_ascii_case(&corrected)
        }) {
            continue;
        }
        out.push(LearnedCorrection {
            heard,
            corrected,
            hits: item.hits.clamp(1, 1_000_000),
        });
        if out.len() >= 256 {
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
    if std::env::var("DICTUM_ORT_INTRA_THREADS").is_err() {
        if settings.ort_intra_threads > 0 {
            std::env::set_var("DICTUM_ORT_INTRA_THREADS", settings.ort_intra_threads.to_string());
        }
    }
    if std::env::var("DICTUM_ORT_INTER_THREADS").is_err() {
        if settings.ort_inter_threads > 0 {
            std::env::set_var("DICTUM_ORT_INTER_THREADS", settings.ort_inter_threads.to_string());
        }
    }
    if std::env::var("DICTUM_ORT_PARALLEL").is_err() {
        std::env::set_var(
            "DICTUM_ORT_PARALLEL",
            if settings.ort_parallel { "1" } else { "0" },
        );
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
            if settings.cloud_mode == "local_only" {
                "0"
            } else {
                "1"
            },
        );
    }
    if std::env::var("DICTUM_CLOUD_MODE").is_err() {
        std::env::set_var("DICTUM_CLOUD_MODE", &settings.cloud_mode);
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
        std::env::set_var(
            "DICTUM_PHRASE_BIAS_TERMS",
            settings.phrase_bias_terms.join("\n"),
        );
    }
    if std::env::var("DICTUM_RELIABILITY_MODE").is_err() {
        std::env::set_var(
            "DICTUM_RELIABILITY_MODE",
            if settings.reliability_mode { "1" } else { "0" },
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
