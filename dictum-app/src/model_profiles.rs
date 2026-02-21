//! Model profile metadata and hardware-based recommendation helpers.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfileMetadata {
    pub profile: String,
    pub label: String,
    pub speed_tier: String,
    pub quality_tier: String,
    pub min_ram_gb: u64,
    pub min_vram_gb: Option<u64>,
    pub english_optimized: bool,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfileRecommendation {
    pub recommended_profile: String,
    pub suggested_ort_ep: String,
    pub reason: String,
    pub cpu_threads: usize,
    pub system_ram_gb: Option<u64>,
    pub directml_available: bool,
    pub available_profiles: Vec<String>,
}

pub fn model_profile_catalog() -> Vec<ModelProfileMetadata> {
    vec![
        ModelProfileMetadata {
            profile: "large-v3-turbo".into(),
            label: "Large v3 Turbo".into(),
            speed_tier: "fast".into(),
            quality_tier: "high".into(),
            min_ram_gb: 16,
            min_vram_gb: Some(8),
            english_optimized: false,
            notes: "Best quality/speed balance on modern GPUs.".into(),
        },
        ModelProfileMetadata {
            profile: "distil-large-v3".into(),
            label: "Distil Large v3".into(),
            speed_tier: "fast".into(),
            quality_tier: "high".into(),
            min_ram_gb: 12,
            min_vram_gb: Some(6),
            english_optimized: true,
            notes: "Strong English quality with lower latency than full large models.".into(),
        },
        ModelProfileMetadata {
            profile: "large-v3".into(),
            label: "Large v3".into(),
            speed_tier: "medium".into(),
            quality_tier: "max".into(),
            min_ram_gb: 20,
            min_vram_gb: Some(10),
            english_optimized: false,
            notes: "Highest local quality, heaviest compute profile.".into(),
        },
        ModelProfileMetadata {
            profile: "medium.en".into(),
            label: "Medium English".into(),
            speed_tier: "medium".into(),
            quality_tier: "high".into(),
            min_ram_gb: 10,
            min_vram_gb: Some(4),
            english_optimized: true,
            notes: "High quality English profile for balanced CPU/GPU use.".into(),
        },
        ModelProfileMetadata {
            profile: "medium".into(),
            label: "Medium".into(),
            speed_tier: "medium".into(),
            quality_tier: "high".into(),
            min_ram_gb: 10,
            min_vram_gb: Some(4),
            english_optimized: false,
            notes: "Balanced multilingual profile.".into(),
        },
        ModelProfileMetadata {
            profile: "small.en".into(),
            label: "Small English".into(),
            speed_tier: "fast".into(),
            quality_tier: "medium".into(),
            min_ram_gb: 8,
            min_vram_gb: Some(3),
            english_optimized: true,
            notes: "Good CPU fallback with solid English quality.".into(),
        },
        ModelProfileMetadata {
            profile: "small".into(),
            label: "Small".into(),
            speed_tier: "fast".into(),
            quality_tier: "medium".into(),
            min_ram_gb: 8,
            min_vram_gb: Some(3),
            english_optimized: false,
            notes: "Balanced multilingual fallback.".into(),
        },
        ModelProfileMetadata {
            profile: "base.en".into(),
            label: "Base English".into(),
            speed_tier: "very_fast".into(),
            quality_tier: "entry".into(),
            min_ram_gb: 6,
            min_vram_gb: Some(2),
            english_optimized: true,
            notes: "Low-latency English fallback for weaker systems.".into(),
        },
        ModelProfileMetadata {
            profile: "base".into(),
            label: "Base".into(),
            speed_tier: "very_fast".into(),
            quality_tier: "entry".into(),
            min_ram_gb: 6,
            min_vram_gb: Some(2),
            english_optimized: false,
            notes: "Low-latency multilingual fallback.".into(),
        },
        ModelProfileMetadata {
            profile: "tiny.en".into(),
            label: "Tiny English".into(),
            speed_tier: "ultra_fast".into(),
            quality_tier: "basic".into(),
            min_ram_gb: 4,
            min_vram_gb: Some(1),
            english_optimized: true,
            notes: "Fastest English profile, lowest quality.".into(),
        },
        ModelProfileMetadata {
            profile: "tiny".into(),
            label: "Tiny".into(),
            speed_tier: "ultra_fast".into(),
            quality_tier: "basic".into(),
            min_ram_gb: 4,
            min_vram_gb: Some(1),
            english_optimized: false,
            notes: "Fastest multilingual profile, lowest quality.".into(),
        },
    ]
}

pub fn recommend_model_profile(current_ort_ep: &str) -> ModelProfileRecommendation {
    let cpu_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    let system_ram_gb = detect_system_ram_gb();
    let directml_available = cfg!(target_os = "windows")
        && !current_ort_ep.trim().eq_ignore_ascii_case("cpu");
    let suggested_ort_ep = if directml_available {
        "directml"
    } else {
        "cpu"
    };

    let available_profiles = detected_available_profiles();
    let all_profiles = model_profile_catalog()
        .into_iter()
        .map(|m| m.profile)
        .collect::<Vec<_>>();
    let profile_pool = if available_profiles.is_empty() {
        all_profiles.clone()
    } else {
        available_profiles.clone()
    };

    let recommended_profile = choose_profile(
        &profile_pool,
        cpu_threads,
        system_ram_gb.unwrap_or(16),
        directml_available,
    )
    .unwrap_or_else(|| "distil-large-v3".to_string());

    let ram_part = system_ram_gb
        .map(|gb| format!("{gb} GB RAM"))
        .unwrap_or_else(|| "unknown RAM".to_string());
    let reason = if directml_available {
        format!(
            "Detected {cpu_threads} logical cores and {ram_part}; recommending {recommended_profile} for GPU-accelerated dictation."
        )
    } else {
        format!(
            "Detected {cpu_threads} logical cores and {ram_part}; recommending {recommended_profile} for CPU-first stability."
        )
    };

    ModelProfileRecommendation {
        recommended_profile,
        suggested_ort_ep: suggested_ort_ep.into(),
        reason,
        cpu_threads,
        system_ram_gb,
        directml_available,
        available_profiles,
    }
}

fn choose_profile(
    available: &[String],
    cpu_threads: usize,
    ram_gb: u64,
    directml_available: bool,
) -> Option<String> {
    // Distil Large v3 is the project-wide recommended profile.
    if available.iter().any(|p| p == "distil-large-v3") {
        return Some("distil-large-v3".to_string());
    }

    let mut candidates: Vec<&str> = Vec::new();
    if directml_available {
        if cpu_threads >= 20 && ram_gb >= 24 {
            candidates.extend(["large-v3-turbo", "distil-large-v3", "large-v3"]);
        } else if cpu_threads >= 12 && ram_gb >= 14 {
            candidates.extend(["distil-large-v3", "large-v3-turbo", "medium.en"]);
        } else {
            candidates.extend(["medium.en", "small.en", "base.en"]);
        }
    } else if cpu_threads >= 16 && ram_gb >= 20 {
        candidates.extend(["medium.en", "small.en", "base.en"]);
    } else if cpu_threads >= 8 && ram_gb >= 10 {
        candidates.extend(["small.en", "base.en", "tiny.en"]);
    } else {
        candidates.extend(["base.en", "tiny.en", "tiny"]);
    }

    for candidate in candidates {
        if available.iter().any(|p| p == candidate) {
            return Some(candidate.to_string());
        }
    }
    available.first().cloned()
}

fn detected_available_profiles() -> Vec<String> {
    let root = dictum_core::inference::onnx::default_models_dir();
    let mut available = Vec::new();
    for profile in model_profile_catalog()
        .into_iter()
        .map(|m| m.profile)
        .collect::<Vec<_>>()
    {
        if model_profile_exists(&root, &profile) {
            available.push(profile);
        }
    }
    available
}

fn model_profile_exists(models_root: &std::path::Path, profile: &str) -> bool {
    let required_in = |dir: &std::path::Path| {
        dir.join("encoder_model.onnx").exists()
            && dir.join("decoder_model.onnx").exists()
            && dir.join("tokenizer.json").exists()
    };
    let profile_dir = models_root.join(profile);
    if required_in(&profile_dir) {
        return true;
    }
    profile == "small" && required_in(models_root)
}

#[cfg(target_os = "windows")]
fn detect_system_ram_gb() -> Option<u64> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    unsafe {
        let mut status: MEMORYSTATUSEX = zeroed();
        status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut status) == 0 {
            return None;
        }
        Some((status.ullTotalPhys as u64) / (1024 * 1024 * 1024))
    }
}

#[cfg(not(target_os = "windows"))]
fn detect_system_ram_gb() -> Option<u64> {
    None
}
