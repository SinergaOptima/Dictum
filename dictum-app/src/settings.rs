//! Persistent application settings (JSON file in app data directory).

use std::fs;
use std::path::{Path, PathBuf};

use dictum_core::{engine::EngineConfig, DictumEngine};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedCorrection {
    pub heard: String,
    pub corrected: String,
    pub hits: usize,
    #[serde(default)]
    pub mode_affinity: Option<String>,
    #[serde(default)]
    pub app_profile_affinity: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppProfile {
    pub id: String,
    pub name: String,
    pub app_match: String,
    pub dictation_mode: String,
    pub phrase_bias_terms: Vec<String>,
    pub post_utterance_refine: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AppSettings {
    pub preferred_input_device: Option<String>,
    pub model_profile: String,
    pub performance_profile: String,
    pub dictation_mode: String,
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
    pub app_profiles: Vec<AppProfile>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            preferred_input_device: None,
            model_profile: "distil-large-v3".into(),
            performance_profile: "whisper_balanced_english".into(),
            dictation_mode: "conversation".into(),
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
            app_profiles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSettings {
    pub model_profile: String,
    pub performance_profile: String,
    pub dictation_mode: String,
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
    pub app_profile_count: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RuntimeEnvMode {
    FillMissing,
    Overwrite,
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.model_profile = normalize_model_profile(&self.model_profile);
        self.performance_profile = normalize_performance_profile(&self.performance_profile);
        self.dictation_mode = normalize_dictation_mode(&self.dictation_mode);
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
        self.app_profiles = normalize_app_profiles(&self.app_profiles);
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
            dictation_mode: self.dictation_mode.clone(),
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
            app_profile_count: self.app_profiles.len(),
        }
    }
}

pub fn apply_engine_profile(config: &mut EngineConfig, profile: &str) {
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

pub fn engine_config_for_settings(settings: &AppSettings) -> EngineConfig {
    let mut config = EngineConfig::default();
    apply_engine_profile(&mut config, &settings.performance_profile);
    config
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

pub fn normalize_dictation_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "coding" | "code" => "coding".into(),
        "command" | "commands" => "command".into(),
        _ => "conversation".into(),
    }
}

fn normalize_executable_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return None;
    }
    let basename = trimmed
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(trimmed)
        .trim()
        .to_ascii_lowercase();
    if basename.is_empty() {
        return None;
    }
    if basename.ends_with(".exe") {
        return Some(basename);
    }
    if !basename.contains('.') {
        return Some(format!("{basename}.exe"));
    }
    None
}

fn normalize_app_profiles(profiles: &[AppProfile]) -> Vec<AppProfile> {
    let mut out = Vec::new();
    for profile in profiles {
        let id = profile.id.trim().to_string();
        let name = profile.name.trim().to_string();
        let Some(app_match) = normalize_executable_name(&profile.app_match) else {
            continue;
        };
        if id.is_empty() || name.is_empty() || app_match.is_empty() {
            continue;
        }
        out.push(AppProfile {
            id,
            name,
            app_match,
            dictation_mode: normalize_dictation_mode(&profile.dictation_mode),
            phrase_bias_terms: normalize_phrase_bias_terms(&profile.phrase_bias_terms),
            post_utterance_refine: profile.post_utterance_refine,
            enabled: profile.enabled,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.app_match.cmp(&b.app_match)));
    out
}

pub fn resolve_app_profile<'a>(
    settings: &'a AppSettings,
    foreground_app: Option<&str>,
) -> Option<&'a AppProfile> {
    let Some(app) = foreground_app.and_then(normalize_executable_name) else {
        return None;
    };
    settings
        .app_profiles
        .iter()
        .find(|profile| profile.enabled && profile.app_match == app)
}

pub fn apply_runtime_env_with_profile(
    settings: &AppSettings,
    profile: Option<&AppProfile>,
    mode: RuntimeEnvMode,
) {
    apply_runtime_env_from_settings(settings, mode);
    if let Some(profile) = profile {
        set_runtime_var(
            "DICTUM_POST_UTTERANCE_REFINEMENT",
            Some(
                if profile.post_utterance_refine {
                    "1"
                } else {
                    "0"
                }
                .to_string(),
            ),
            RuntimeEnvMode::Overwrite,
        );
        set_runtime_var(
            "DICTUM_PHRASE_BIAS_TERMS",
            Some(profile.phrase_bias_terms.join("\n")),
            RuntimeEnvMode::Overwrite,
        );
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
            e.heard.eq_ignore_ascii_case(&heard)
                && e.corrected.eq_ignore_ascii_case(&corrected)
                && e.mode_affinity == item.mode_affinity
                && e.app_profile_affinity == item.app_profile_affinity
        }) {
            continue;
        }
        out.push(LearnedCorrection {
            heard,
            corrected,
            hits: item.hits.clamp(1, 1_000_000),
            mode_affinity: item
                .mode_affinity
                .as_ref()
                .map(|value| normalize_dictation_mode(value))
                .filter(|value| !value.is_empty()),
            app_profile_affinity: item
                .app_profile_affinity
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            last_used_at: item
                .last_used_at
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        });
        if out.len() >= 256 {
            break;
        }
    }
    out.sort_by(|a, b| {
        b.hits
            .cmp(&a.hits)
            .then_with(|| b.last_used_at.cmp(&a.last_used_at))
            .then_with(|| a.heard.cmp(&b.heard))
    });
    out
}

fn set_runtime_var(name: &str, value: Option<String>, mode: RuntimeEnvMode) {
    if mode == RuntimeEnvMode::FillMissing && std::env::var(name).is_ok() {
        return;
    }

    match value {
        Some(value) => std::env::set_var(name, value),
        None if mode == RuntimeEnvMode::Overwrite => std::env::remove_var(name),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_app_profiles, normalize_learned_corrections, resolve_app_profile, AppProfile,
        AppSettings, LearnedCorrection,
    };

    #[test]
    fn normalize_learned_corrections_keeps_distinct_context_variants() {
        let normalized = normalize_learned_corrections(&[
            LearnedCorrection {
                heard: "printf".into(),
                corrected: "println!".into(),
                hits: 1,
                mode_affinity: Some("coding".into()),
                app_profile_affinity: None,
                last_used_at: Some("2026-03-07T10:00:00Z".into()),
            },
            LearnedCorrection {
                heard: "printf".into(),
                corrected: "println!".into(),
                hits: 1,
                mode_affinity: Some("conversation".into()),
                app_profile_affinity: None,
                last_used_at: Some("2026-03-07T11:00:00Z".into()),
            },
        ]);

        assert_eq!(normalized.len(), 2);
    }

    #[test]
    fn normalize_learned_corrections_deduplicates_same_context_rule() {
        let normalized = normalize_learned_corrections(&[
            LearnedCorrection {
                heard: "ladder labs".into(),
                corrected: "Lattice Labs".into(),
                hits: 1,
                mode_affinity: Some("conversation".into()),
                app_profile_affinity: Some("slack-profile".into()),
                last_used_at: Some("2026-03-07T10:00:00Z".into()),
            },
            LearnedCorrection {
                heard: "LADDER LABS".into(),
                corrected: "Lattice Labs".into(),
                hits: 5,
                mode_affinity: Some("conversation".into()),
                app_profile_affinity: Some("slack-profile".into()),
                last_used_at: Some("2026-03-07T11:00:00Z".into()),
            },
        ]);

        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].heard, "ladder labs");
        assert_eq!(normalized[0].mode_affinity.as_deref(), Some("conversation"));
        assert_eq!(
            normalized[0].app_profile_affinity.as_deref(),
            Some("slack-profile")
        );
    }

    #[test]
    fn normalize_app_profiles_reduces_windows_paths_to_executable_name() {
        let normalized = normalize_app_profiles(&[AppProfile {
            id: "cursor".into(),
            name: "Cursor".into(),
            app_match: r#""C:\Program Files\Cursor\Cursor.exe""#.into(),
            dictation_mode: "coding".into(),
            phrase_bias_terms: vec!["TypeScript".into()],
            post_utterance_refine: true,
            enabled: true,
        }]);

        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].app_match, "cursor.exe");
    }

    #[test]
    fn resolve_app_profile_matches_normalized_foreground_basename() {
        let mut settings = AppSettings::default();
        settings.app_profiles = vec![AppProfile {
            id: "cursor".into(),
            name: "Cursor".into(),
            app_match: "cursor.exe".into(),
            dictation_mode: "coding".into(),
            phrase_bias_terms: Vec::new(),
            post_utterance_refine: false,
            enabled: true,
        }];
        settings.normalize();

        let matched = resolve_app_profile(
            &settings,
            Some(r"C:\Program Files\Cursor\resources\app\Cursor.exe"),
        )
        .expect("profile match");
        assert_eq!(matched.id, "cursor");
    }

    #[test]
    fn resolve_app_profile_does_not_match_partial_substrings() {
        let mut settings = AppSettings::default();
        settings.app_profiles = vec![AppProfile {
            id: "code".into(),
            name: "VS Code".into(),
            app_match: "code.exe".into(),
            dictation_mode: "coding".into(),
            phrase_bias_terms: Vec::new(),
            post_utterance_refine: false,
            enabled: true,
        }];
        settings.normalize();

        assert!(resolve_app_profile(&settings, Some("mycode-helper.exe")).is_none());
    }
}

pub fn apply_runtime_env_from_settings(settings: &AppSettings, mode: RuntimeEnvMode) {
    set_runtime_var(
        "DICTUM_MODEL_PROFILE",
        Some(settings.model_profile.clone()),
        mode,
    );
    set_runtime_var("DICTUM_ORT_EP", Some(settings.ort_ep.clone()), mode);
    set_runtime_var(
        "DICTUM_ORT_INTRA_THREADS",
        (settings.ort_intra_threads > 0).then(|| settings.ort_intra_threads.to_string()),
        mode,
    );
    set_runtime_var(
        "DICTUM_ORT_INTER_THREADS",
        (settings.ort_inter_threads > 0).then(|| settings.ort_inter_threads.to_string()),
        mode,
    );
    set_runtime_var(
        "DICTUM_ORT_PARALLEL",
        Some(if settings.ort_parallel { "1" } else { "0" }.to_string()),
        mode,
    );
    set_runtime_var(
        "DICTUM_PERFORMANCE_PROFILE",
        Some(settings.performance_profile.clone()),
        mode,
    );
    set_runtime_var(
        "DICTUM_LANGUAGE_HINT",
        Some(settings.language_hint.clone()),
        mode,
    );
    set_runtime_var(
        "DICTUM_CLOUD_FALLBACK",
        Some(
            if settings.cloud_mode == "local_only" {
                "0"
            } else {
                "1"
            }
            .to_string(),
        ),
        mode,
    );
    set_runtime_var("DICTUM_CLOUD_MODE", Some(settings.cloud_mode.clone()), mode);
    set_runtime_var(
        "DICTUM_OPENAI_API_KEY",
        settings.openai_api_key.clone(),
        mode,
    );
    set_runtime_var(
        "DICTUM_INPUT_GAIN_BOOST",
        Some(format!("{:.4}", settings.input_gain_boost)),
        mode,
    );
    set_runtime_var(
        "DICTUM_POST_UTTERANCE_REFINEMENT",
        Some(
            if settings.post_utterance_refine {
                "1"
            } else {
                "0"
            }
            .to_string(),
        ),
        mode,
    );
    set_runtime_var(
        "DICTUM_PHRASE_BIAS_TERMS",
        Some(settings.phrase_bias_terms.join("\n")),
        mode,
    );
    set_runtime_var(
        "DICTUM_RELIABILITY_MODE",
        Some(if settings.reliability_mode { "1" } else { "0" }.to_string()),
        mode,
    );
}

pub fn sync_runtime_with_settings(
    engine: &DictumEngine,
    settings: &AppSettings,
    env_mode: RuntimeEnvMode,
) {
    engine.update_config(engine_config_for_settings(settings));
    apply_runtime_env_from_settings(settings, env_mode);
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
