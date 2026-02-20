//! Silero VAD neural voice activity detector.
//!
//! Wraps the official Silero VAD ONNX model published at
//! <https://github.com/snakers4/silero-vad>.
//!
//! Supports both the v3/v4 LSTM interface (separate `h`/`c` tensors) and the
//! v5 GRU interface (single `state` tensor).
//!
//! ## Model I/O (v4 LSTM)
//!
//! | Name     | Shape      | DType | Direction |
//! |----------|------------|-------|-----------|
//! | `input`  | `[1, 512]` | f32   | in        |
//! | `sr`     | `[1]`      | i64   | in        |
//! | `h`      | `[2,1,64]` | f32   | in/out    |
//! | `c`      | `[2,1,64]` | f32   | in/out    |
//! | `output` | `[1, 1]`   | f32   | out       |
//! | `hn`     | `[2,1,64]` | f32   | out       |
//! | `cn`     | `[2,1,64]` | f32   | out       |
//!
//! ## Model I/O (v5 GRU)
//!
//! | Name     | Shape      | DType | Direction |
//! |----------|------------|-------|-----------|
//! | `input`  | `[1, 512]` | f32   | in        |
//! | `sr`     | `[1]`      | i64   | in        |
//! | `state`  | `[2,1,64]` | f32   | in/out    |
//! | `output` | `[1, 1]`   | f32   | out       |
//! | `stateN` | `[2,1,64]` | f32   | out       |

use std::path::PathBuf;

use ndarray::{Array1, Array2, Array3};
use ort::session::builder::SessionBuilder;
use ort::session::SessionInputValue;
use ort::value::Value;
use tracing::{error, info, warn};

use super::{VadDecision, VoiceActivityDetector};
use crate::inference::onnx::default_models_dir;
use crate::{
    buffering::chunk::AudioChunk,
    error::{DictumError, Result},
};

/// Window size expected by Silero VAD (samples at 16 kHz = 32 ms).
const WINDOW: usize = 512;
/// v3/v4 LSTM state size: 2 layers × 1 batch × 64 units = 128 floats (each of h and c).
const LSTM_SIZE: usize = 128;
/// v5 GRU state size: 2 layers × 1 batch × 128 units = 256 floats.
const GRU_STATE_SIZE: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SileroIoMode {
    /// v3/v4 LSTM: separate `h` [2,1,64] and `c` [2,1,64] state tensors.
    StatefulLstm,
    /// v5 GRU: single `state` [2,1,64] tensor, output `stateN`.
    StatefulGru,
    /// No state passing (stateless fallback).
    Stateless,
}

/// Neural VAD using Silero VAD ONNX model (v3/v4 LSTM or v5 GRU).
pub struct SileroVad {
    session: ort::session::Session,
    io_mode: SileroIoMode,
    input_name: String,
    sr_name: Option<String>,
    output_name: String,
    // v3/v4 LSTM state names
    h_name: Option<String>,
    c_name: Option<String>,
    hn_name: Option<String>,
    cn_name: Option<String>,
    // v5 GRU state names
    state_name: Option<String>,
    state_out_name: Option<String>,
    // state buffers
    h: Vec<f32>,     // [2, 1, 64] row-major (LSTM h)
    c: Vec<f32>,     // [2, 1, 64] row-major (LSTM c)
    state: Vec<f32>, // [2, 1, 64] row-major (GRU state)
    threshold: f32,
    input_buf: Vec<f32>,
}

impl SileroVad {
    /// Load the Silero VAD ONNX model from `path` with the given `threshold`.
    pub fn new(path: impl AsRef<std::path::Path>, threshold: f32) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(DictumError::ModelNotFound {
                path: path.to_path_buf(),
            });
        }

        let metadata = std::fs::metadata(path);
        let size_mb = metadata
            .map(|m| m.len() as f64 / 1_048_576.0)
            .unwrap_or(0.0);

        info!("=== SileroVad Startup Report ===");
        info!("  path: {:?}", path);
        info!("  size: {:.2} MB", size_mb);
        info!("  threshold: {}", threshold);

        let session = SessionBuilder::new()
            .map_err(|e| DictumError::OnnxSession(e.to_string()))?
            .commit_from_file(path)
            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;

        let input_names: Vec<String> = session
            .inputs()
            .iter()
            .map(|outlet| outlet.name().to_string())
            .collect();
        let output_names: Vec<String> = session
            .outputs()
            .iter()
            .map(|outlet| outlet.name().to_string())
            .collect();

        info!("  inputs: {:?}", input_names);
        info!("  outputs: {:?}", output_names);

        let input_name = resolve_name(&input_names, &["input", "audio", "x"])
            .or_else(|| input_names.first().cloned())
            .ok_or_else(|| DictumError::OnnxSession("Silero model has no inputs".into()))?;
        let sr_name = resolve_name(&input_names, &["sr", "sample_rate"]);

        // v3/v4 LSTM state tensors
        let h_name = resolve_name(&input_names, &["h", "state_h"]);
        let c_name = resolve_name(&input_names, &["c", "state_c"]);

        // v5 GRU combined state tensor
        let state_name = resolve_name(&input_names, &["state", "h_0", "hidden"]);

        let output_name = resolve_name(&output_names, &["output", "speech_prob", "prob"])
            .or_else(|| output_names.first().cloned())
            .ok_or_else(|| DictumError::OnnxSession("Silero model has no outputs".into()))?;
        let hn_name = resolve_name(&output_names, &["hn", "state_hn", "h_out"]);
        let cn_name = resolve_name(&output_names, &["cn", "state_cn", "c_out"]);
        let state_out_name =
            resolve_name(&output_names, &["stateN", "state_out", "h_0_out", "hn_out"]);

        let io_mode =
            if h_name.is_some() && c_name.is_some() && hn_name.is_some() && cn_name.is_some() {
                SileroIoMode::StatefulLstm
            } else if state_name.is_some() {
                SileroIoMode::StatefulGru
            } else {
                SileroIoMode::Stateless
            };

        info!("  io_mode: {:?}", io_mode);
        info!("=== SileroVad ready ===");

        Ok(Self {
            session,
            io_mode,
            input_name,
            sr_name,
            output_name,
            h_name,
            c_name,
            hn_name,
            cn_name,
            state_name,
            state_out_name,
            h: vec![0.0; LSTM_SIZE],
            c: vec![0.0; LSTM_SIZE],
            state: vec![0.0; GRU_STATE_SIZE],
            threshold,
            input_buf: Vec::new(),
        })
    }

    /// Default path for the Silero VAD model file.
    pub fn default_model_path() -> PathBuf {
        default_models_dir().join("silero_vad.onnx")
    }

    /// Run one 512-sample window through the model; update h/c; return speech probability.
    fn run_window(&mut self, window: &[f32]) -> Result<f32> {
        debug_assert_eq!(window.len(), WINDOW);

        let input_arr = Array2::<f32>::from_shape_vec((1, WINDOW), window.to_vec())
            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
        let input_val = Value::from_array(input_arr)
            .map_err(|e: ort::Error| DictumError::OnnxSession(e.to_string()))?;

        let mut input_values: Vec<(String, SessionInputValue<'_>)> =
            vec![(self.input_name.clone(), input_val.into())];

        if self.sr_name.is_some() {
            let sr_arr = Array1::<i64>::from_elem(1, 16_000i64);
            let sr_val = Value::from_array(sr_arr)
                .map_err(|e: ort::Error| DictumError::OnnxSession(e.to_string()))?;
            input_values.push((
                self.sr_name.as_ref().cloned().unwrap_or_default(),
                sr_val.into(),
            ));
        }

        match self.io_mode {
            SileroIoMode::StatefulLstm => {
                let h_arr = Array3::<f32>::from_shape_vec((2, 1, 64), self.h.clone())
                    .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                let c_arr = Array3::<f32>::from_shape_vec((2, 1, 64), self.c.clone())
                    .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                let h_val = Value::from_array(h_arr)
                    .map_err(|e: ort::Error| DictumError::OnnxSession(e.to_string()))?;
                let c_val = Value::from_array(c_arr)
                    .map_err(|e: ort::Error| DictumError::OnnxSession(e.to_string()))?;
                if let Some(h_name) = &self.h_name {
                    input_values.push((h_name.clone(), h_val.into()));
                }
                if let Some(c_name) = &self.c_name {
                    input_values.push((c_name.clone(), c_val.into()));
                }
            }
            SileroIoMode::StatefulGru => {
                let state_arr = Array3::<f32>::from_shape_vec((2, 1, 128), self.state.clone())
                    .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                let state_val = Value::from_array(state_arr)
                    .map_err(|e: ort::Error| DictumError::OnnxSession(e.to_string()))?;
                if let Some(state_name) = &self.state_name {
                    input_values.push((state_name.clone(), state_val.into()));
                }
            }
            SileroIoMode::Stateless => {}
        }

        let outputs = self
            .session
            .run(input_values)
            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;

        // Speech probability scalar from output [1, 1] (or closest available scalar output)
        let prob_output = outputs
            .get(self.output_name.as_str())
            .unwrap_or(&outputs[0]);
        let (_, prob_data) = prob_output
            .try_extract_tensor::<f32>()
            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
        let prob = prob_data.first().copied().unwrap_or(0.0);

        // Update state from model outputs
        match self.io_mode {
            SileroIoMode::StatefulLstm => match (self.hn_name.as_ref(), self.cn_name.as_ref()) {
                (Some(hn_name), Some(cn_name)) => {
                    if let (Some(hn_out), Some(cn_out)) =
                        (outputs.get(hn_name.as_str()), outputs.get(cn_name.as_str()))
                    {
                        let (_, hn_data) = hn_out
                            .try_extract_tensor::<f32>()
                            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                        let (_, cn_data) = cn_out
                            .try_extract_tensor::<f32>()
                            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                        self.h = hn_data.to_vec();
                        self.c = cn_data.to_vec();
                    } else {
                        warn!("SileroVad LSTM state outputs missing; switching to stateless");
                        self.io_mode = SileroIoMode::Stateless;
                    }
                }
                _ => {
                    self.io_mode = SileroIoMode::Stateless;
                }
            },
            SileroIoMode::StatefulGru => {
                if let Some(state_out_name) = &self.state_out_name {
                    if let Some(state_out) = outputs.get(state_out_name.as_str()) {
                        let (_, state_data) = state_out
                            .try_extract_tensor::<f32>()
                            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                        self.state = state_data.to_vec();
                    } else {
                        warn!("SileroVad GRU state output missing; switching to stateless");
                        self.io_mode = SileroIoMode::Stateless;
                    }
                }
            }
            SileroIoMode::Stateless => {}
        }

        Ok(prob)
    }
}

fn resolve_name(candidates: &[String], preferred: &[&str]) -> Option<String> {
    preferred.iter().find_map(|needle| {
        candidates
            .iter()
            .find(|name| name.eq_ignore_ascii_case(needle))
            .cloned()
    })
}

impl VoiceActivityDetector for SileroVad {
    fn classify(&mut self, chunk: &AudioChunk) -> VadDecision {
        self.input_buf.extend_from_slice(&chunk.samples);

        let mut any_speech = false;

        while self.input_buf.len() >= WINDOW {
            let window: Vec<f32> = self.input_buf[..WINDOW].to_vec();
            self.input_buf.drain(..WINDOW);

            match self.run_window(&window) {
                Ok(prob) if prob >= self.threshold => {
                    any_speech = true;
                }
                Ok(_) => {}
                Err(e) => {
                    error!("SileroVad inference error: {e}");
                }
            }
        }

        if any_speech {
            VadDecision::Speech
        } else {
            VadDecision::Silence
        }
    }

    fn reset(&mut self) {
        self.h.iter_mut().for_each(|v| *v = 0.0);
        self.c.iter_mut().for_each(|v| *v = 0.0);
        self.state.iter_mut().for_each(|v| *v = 0.0);
        self.input_buf.clear();
    }
}
