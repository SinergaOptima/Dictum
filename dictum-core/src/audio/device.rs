//! Audio device enumeration.

use serde::{Deserialize, Serialize};

/// Metadata about an audio input device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Human-readable device name reported by the OS.
    pub name: String,
    /// Whether this is the system default input device.
    pub is_default: bool,
    /// Heuristic flag for devices that likely capture system/output audio.
    pub is_loopback_like: bool,
    /// Heuristic recommendation for best speech microphone input.
    pub is_recommended: bool,
}

const LOOPBACK_KEYWORDS: &[&str] = &[
    "stereo mix",
    "wave out",
    "what u hear",
    "what you hear",
    "loopback",
    "virtual output",
    "monitor of",
    "mixage stereo",
    "mezcla estereo",
    "mix stereo",
    "speakers (",
    "headphones (",
];

const MIC_POSITIVE_KEYWORDS: &[&str] = &[
    "microphone",
    "mic",
    "array",
    "headset",
    "headphone mic",
    "input",
    "line in",
    "usb",
    "webcam",
    "yeti",
    "podcast",
];

/// Best-effort heuristic for Windows-style loopback/system-output capture devices.
pub fn is_loopback_like_name(name: &str) -> bool {
    let lowered = name.trim().to_ascii_lowercase();
    LOOPBACK_KEYWORDS.iter().any(|k| lowered.contains(k))
}

/// Score a device name for likely speech microphone quality/intent.
///
/// Higher is better. Non-loopback devices should be preferred.
pub fn mic_preference_score(name: &str) -> i32 {
    let lowered = name.trim().to_ascii_lowercase();
    let mut score = 0;
    if !is_loopback_like_name(&lowered) {
        score += 8;
    } else {
        score -= 16;
    }
    if MIC_POSITIVE_KEYWORDS.iter().any(|k| lowered.contains(k)) {
        score += 6;
    }
    if lowered.contains("default") {
        score += 1;
    }
    score
}

/// List all available audio input devices on the system.
///
/// Returns an empty `Vec` if cpal is not available or no devices exist.
#[cfg(feature = "audio-cpal")]
pub fn list_input_devices() -> Vec<DeviceInfo> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    match host.input_devices() {
        Ok(devices) => {
            let mut list = devices
                .enumerate()
                .map(|d| {
                    let (idx, device) = d;
                    let name = device
                        .name()
                        .unwrap_or_else(|_| format!("Input Device {}", idx + 1));
                    let is_default = default_name.as_deref() == Some(name.as_str());
                    let is_loopback_like = is_loopback_like_name(&name);
                    DeviceInfo {
                        name,
                        is_default,
                        is_loopback_like,
                        is_recommended: false,
                    }
                })
                .collect::<Vec<_>>();

            if let Some((idx, _)) = list
                .iter()
                .enumerate()
                .max_by_key(|(_, d)| mic_preference_score(&d.name) + if d.is_default { 2 } else { 0 })
            {
                if let Some(best) = list.get_mut(idx) {
                    best.is_recommended = true;
                }
            }

            list.sort_by_key(|d| {
                (
                    !d.is_recommended,
                    d.is_loopback_like,
                    !d.is_default,
                    d.name.to_ascii_lowercase(),
                )
            });
            list
        }
        Err(e) => {
            tracing::warn!("failed to enumerate input devices: {e}");
            if let Some(default) = host.default_input_device() {
                let name = default
                    .name()
                    .unwrap_or_else(|_| "Default Input Device".to_string());
                let is_loopback_like = is_loopback_like_name(&name);
                vec![DeviceInfo {
                    name,
                    is_default: true,
                    is_loopback_like,
                    is_recommended: !is_loopback_like,
                }]
            } else {
                vec![]
            }
        }
    }
}

#[cfg(not(feature = "audio-cpal"))]
pub fn list_input_devices() -> Vec<DeviceInfo> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::{is_loopback_like_name, mic_preference_score};

    #[test]
    fn detects_common_loopback_names() {
        assert!(is_loopback_like_name("Stereo Mix (Realtek Audio)"));
        assert!(is_loopback_like_name("What U Hear (Sound Blaster)"));
        assert!(is_loopback_like_name("Speakers (High Definition Audio Device)"));
    }

    #[test]
    fn scores_mic_higher_than_loopback() {
        let mic = mic_preference_score("Microphone Array (USB PnP Audio Device)");
        let loopback = mic_preference_score("Stereo Mix (Realtek Audio)");
        assert!(mic > loopback);
    }
}
