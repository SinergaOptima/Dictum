//! Audio device enumeration.

use serde::{Deserialize, Serialize};

/// Metadata about an audio input device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Human-readable device name reported by the OS.
    pub name: String,
    /// Whether this is the system default input device.
    pub is_default: bool,
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
        Ok(devices) => devices
            .enumerate()
            .filter_map(|d| {
                let (idx, device) = d;
                let name = device
                    .name()
                    .unwrap_or_else(|_| format!("Input Device {}", idx + 1));
                let is_default = default_name.as_deref() == Some(name.as_str());
                Some(DeviceInfo { name, is_default })
            })
            .collect(),
        Err(e) => {
            tracing::warn!("failed to enumerate input devices: {e}");
            if let Some(default) = host.default_input_device() {
                let name = default
                    .name()
                    .unwrap_or_else(|_| "Default Input Device".to_string());
                vec![DeviceInfo {
                    name,
                    is_default: true,
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
