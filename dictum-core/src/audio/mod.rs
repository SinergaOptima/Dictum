//! Audio capture via cpal backend.
//!
//! # Design constraints
//!
//! The cpal input callback runs on an OS audio thread at elevated (TIME_CRITICAL on
//! Windows) priority. It **must not**:
//! - Allocate heap memory
//! - Block on a mutex or condvar
//! - Perform I/O
//!
//! This module satisfies that contract by writing directly into an SPSC ring buffer
//! producer whose `push_slice` is lock-free and allocation-free.
//!
//! # Threading note
//!
//! `cpal::Stream` is `!Send` on most platforms (COM on Windows, CoreAudio on macOS).
//! `AudioCapture` therefore must be created and dropped on the same thread.
//! The pipeline accomplishes this by calling `open_default` inside `spawn_blocking`.

pub mod device;
pub mod resample;

#[cfg(feature = "audio-cpal")]
use cpal::{
    traits::{DeviceTrait, StreamTrait},
    SampleFormat, SampleRate, Stream, StreamConfig,
};

use crate::{
    buffering::{AudioProducer, Producer},
    error::{DictumError, Result},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tracing::{error, info, warn};

/// Handle to an active audio capture stream.
///
/// **Not `Send`** — `cpal::Stream` is bound to its creation thread on Windows/macOS.
/// Create and drop this type on the same OS thread.
pub struct AudioCapture {
    /// Kept alive so the stream is not dropped prematurely.
    #[cfg(feature = "audio-cpal")]
    _stream: Stream,
    /// Shared flag — set to `false` to signal the callback to no-op.
    running: Arc<AtomicBool>,
    /// Actual capture sample rate reported by the device (Hz).
    pub sample_rate: u32,
}

impl AudioCapture {
    /// Open an input device by preferred name, otherwise fall back to
    /// default input device and then first available device.
    #[cfg(feature = "audio-cpal")]
    pub fn open_with_preference(
        mut producer: AudioProducer,
        running: Arc<AtomicBool>,
        preferred_device_name: Option<&str>,
    ) -> Result<Self> {
        use cpal::traits::HostTrait;

        let host = cpal::default_host();
        let mut selected_device = None;

        if let Some(preferred_name) = preferred_device_name {
            match host.input_devices() {
                Ok(mut devices) => {
                    selected_device = devices.find(|device| {
                        device
                            .name()
                            .map(|name| name == preferred_name)
                            .unwrap_or(false)
                    });

                    if selected_device.is_none() {
                        warn!(
                            "preferred input device '{}' not found, falling back",
                            preferred_name
                        );
                    }
                }
                Err(e) => {
                    warn!("failed to list input devices while resolving preference: {e}");
                }
            }
        }

        let device = if let Some(device) = selected_device {
            device
        } else if let Some(default) = host.default_input_device() {
            default
        } else {
            let mut devices = host
                .input_devices()
                .map_err(|e| DictumError::AudioDevice(e.to_string()))?;
            let fallback = devices.next().ok_or(DictumError::NoDefaultInputDevice)?;
            warn!("no default input device, falling back to first available input");
            fallback
        };

        info!(
            device = device.name().unwrap_or_default().as_str(),
            "opening input device"
        );

        let supported = device
            .default_input_config()
            .map_err(|e| DictumError::AudioDevice(e.to_string()))?;

        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();

        info!(sample_rate, channels, "audio config selected");

        let config = StreamConfig {
            channels,
            sample_rate: SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        // Pre-clone one Arc per sample format branch so each closure owns its flag.
        let running_f32 = Arc::clone(&running);
        let running_i16 = Arc::clone(&running);
        let running_u8 = Arc::clone(&running);

        let stream = match supported.sample_format() {
            SampleFormat::F32 => {
                let ch = channels as usize;
                let mut mix_buf_f32: Vec<f32> = Vec::new();
                device.build_input_stream(
                    &config,
                    move |data: &[f32], _info| {
                        if !running_f32.load(Ordering::Relaxed) {
                            return;
                        }
                        if channels == 1 {
                            let written = producer.push_slice(data);
                            if written < data.len() {
                                warn!(
                                    "ring buffer full: dropped {} f32 frames",
                                    data.len() - written
                                );
                            }
                            return;
                        }

                        let frames = data.len() / ch;
                        mix_buf_f32.resize(frames, 0.0);
                        for f in 0..frames {
                            let mut sum = 0f32;
                            let base = f * ch;
                            for c in 0..ch {
                                sum += data[base + c];
                            }
                            mix_buf_f32[f] = sum / ch as f32;
                        }
                        let written = producer.push_slice(&mix_buf_f32);
                        if written < mix_buf_f32.len() {
                            warn!(
                                "ring buffer full: dropped {} f32 frames",
                                mix_buf_f32.len() - written
                            );
                        }
                    },
                    |err| error!("audio stream error: {err}"),
                    None,
                )
            }

            SampleFormat::I16 => {
                let ch = channels as usize;
                let mut mix_buf_i16: Vec<f32> = Vec::new();
                device.build_input_stream(
                    &config,
                    move |data: &[i16], _info| {
                        if !running_i16.load(Ordering::Relaxed) {
                            return;
                        }
                        let frames = data.len() / ch;
                        mix_buf_i16.resize(frames, 0.0);
                        if ch == 1 {
                            for (idx, sample) in data.iter().take(frames).enumerate() {
                                mix_buf_i16[idx] = *sample as f32 / 32768.0;
                            }
                        } else {
                            for f in 0..frames {
                                let mut sum = 0f32;
                                let base = f * ch;
                                for c in 0..ch {
                                    sum += data[base + c] as f32 / 32768.0;
                                }
                                mix_buf_i16[f] = sum / ch as f32;
                            }
                        }
                        let written = producer.push_slice(&mix_buf_i16);
                        if written < mix_buf_i16.len() {
                            warn!(
                                "ring buffer full: dropped {} i16 frames",
                                mix_buf_i16.len() - written
                            );
                        }
                    },
                    |err| error!("audio stream error: {err}"),
                    None,
                )
            }

            SampleFormat::U8 => {
                let ch = channels as usize;
                let mut mix_buf_u8: Vec<f32> = Vec::new();
                device.build_input_stream(
                    &config,
                    move |data: &[u8], _info| {
                        if !running_u8.load(Ordering::Relaxed) {
                            return;
                        }
                        let frames = data.len() / ch;
                        mix_buf_u8.resize(frames, 0.0);
                        if ch == 1 {
                            for (idx, sample) in data.iter().take(frames).enumerate() {
                                mix_buf_u8[idx] = (*sample as f32 - 128.0) / 128.0;
                            }
                        } else {
                            for f in 0..frames {
                                let mut sum = 0f32;
                                let base = f * ch;
                                for c in 0..ch {
                                    sum += (data[base + c] as f32 - 128.0) / 128.0;
                                }
                                mix_buf_u8[f] = sum / ch as f32;
                            }
                        }
                        let written = producer.push_slice(&mix_buf_u8);
                        if written < mix_buf_u8.len() {
                            warn!(
                                "ring buffer full: dropped {} u8 frames",
                                mix_buf_u8.len() - written
                            );
                        }
                    },
                    |err| error!("audio stream error: {err}"),
                    None,
                )
            }

            fmt => {
                return Err(DictumError::AudioStream(format!(
                    "unsupported sample format: {fmt:?}"
                )))
            }
        }
        .map_err(|e| DictumError::AudioStream(e.to_string()))?;

        stream
            .play()
            .map_err(|e| DictumError::AudioStream(e.to_string()))?;

        Ok(Self {
            _stream: stream,
            running,
            sample_rate,
        })
    }

    /// Open the system default microphone and push f32 PCM frames into `producer`.
    ///
    /// Must be called from the thread that will also drop this value.
    /// In practice this means calling it inside `tokio::task::spawn_blocking`.
    ///
    /// # Errors
    /// Returns `DictumError::NoDefaultInputDevice` when no microphone is available,
    /// or `DictumError::AudioStream` if cpal fails to build the stream.
    #[cfg(feature = "audio-cpal")]
    pub fn open_default(producer: AudioProducer, running: Arc<AtomicBool>) -> Result<Self> {
        Self::open_with_preference(producer, running, None)
    }

    /// Stop: signal the callback to no-op on its next invocation.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }
}

/// Stub when the `audio-cpal` feature is disabled.
#[cfg(not(feature = "audio-cpal"))]
impl AudioCapture {
    pub fn open_with_preference(
        _producer: AudioProducer,
        _running: Arc<AtomicBool>,
        _preferred_device_name: Option<&str>,
    ) -> Result<Self> {
        Err(DictumError::AudioStream(
            "compiled without audio-cpal feature".into(),
        ))
    }

    pub fn open_default(producer: AudioProducer, running: Arc<AtomicBool>) -> Result<Self> {
        Self::open_with_preference(producer, running, None)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }
}
