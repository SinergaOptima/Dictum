//! Whisper ONNX backend via the `ort` crate.
//!
//! Targets the HuggingFace `optimum` separate encoder + decoder export:
//! - `encoder_model.onnx` — input `[1,80,3000]` → `last_hidden_state [1,1500,384]`
//! - `decoder_model.onnx` — `input_ids [1,seq]` + `encoder_hidden_states [1,1500,384]`
//!   → `logits [1,seq,vocab]`
//! - `tokenizer.json`     — HuggingFace fast tokenizer
//!
//! ## Mel spectrogram parameters (must match training)
//!
//! | Parameter       | Value          |
//! |-----------------|----------------|
//! | Hann window     | 400 samples    |
//! | FFT size        | 400            |
//! | Frequency bins  | 201 (400/2+1)  |
//! | Hop length      | 160 (10 ms)    |
//! | Mel bands       | 80             |
//! | Mel range       | 0–8 000 Hz     |
//! | Frames          | 3 000 (30 s)   |
//!
//! ## Decoder
//!
//! Greedy (argmax) decode with Whisper-style suppression + prefix fallback.
//! Stops at EOT `50257` or 224 tokens. Partial mode caps at 10 steps.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    iter::FromIterator,
};

use ndarray::Array3;
use ort::session::{Session, SessionInputValue, SessionOutputs};
use ort::value::{DynValue, TensorRef, Value};
use ort::{
    ep,
    session::builder::{GraphOptimizationLevel, SessionBuilder},
};
use reqwest::blocking::multipart;
use rustfft::{num_complex::Complex, FftPlanner};
use tokenizers::Tokenizer;
use tracing::{debug, info, warn};

use crate::{
    buffering::chunk::AudioChunk,
    error::{DictumError, Result},
    inference::SpeechModel,
    ipc::events::{SegmentKind, TranscriptSegment},
};

static DEBUG_TRANSCRIBE: OnceLock<bool> = OnceLock::new();
static LANGUAGE_HINT: OnceLock<DecodeLanguageHint> = OnceLock::new();

fn is_debug_transcribe() -> bool {
    *DEBUG_TRANSCRIBE.get_or_init(|| {
        std::env::var("DICTUM_DEBUG_TRANSCRIBE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeLanguageHint {
    Auto,
    English,
    Mandarin,
    Russian,
}

fn decode_language_hint() -> DecodeLanguageHint {
    *LANGUAGE_HINT.get_or_init(|| {
        match std::env::var("DICTUM_LANGUAGE_HINT")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "en" | "eng" | "english" => DecodeLanguageHint::English,
            "zh" | "zh-cn" | "zh-hans" | "mandarin" | "chinese" => DecodeLanguageHint::Mandarin,
            "ru" | "rus" | "russian" => DecodeLanguageHint::Russian,
            _ => DecodeLanguageHint::Auto,
        }
    })
}

fn cloud_fallback_enabled() -> bool {
    std::env::var("DICTUM_CLOUD_FALLBACK")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn post_utterance_refinement_enabled() -> bool {
    std::env::var("DICTUM_POST_UTTERANCE_REFINEMENT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn phrase_bias_terms_from_env() -> Vec<String> {
    std::env::var("DICTUM_PHRASE_BIAS_TERMS")
        .ok()
        .map(|raw| {
            raw.lines()
                .flat_map(|line| line.split(','))
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

// ── Mel spectrogram constants ────────────────────────────────────────────────
const N_FFT: usize = 400;
// Whisper expects an n_fft=400 STFT frontend (201 freq bins).
const FFT_SIZE: usize = N_FFT;
const N_FREQS: usize = FFT_SIZE / 2 + 1; // 201
const HOP: usize = 160;
const N_MELS: usize = 80;
const N_FRAMES: usize = 3_000;
const MEL_SAMPLES: usize = 480_000;

// ── Decoder constants ────────────────────────────────────────────────────────
const EOT: i64 = 50257; // <|endoftext|> for this tokenizer export
const SOT_FALLBACK: i64 = 50258;
const ENGLISH_FALLBACK: i64 = 50259;
const TRANSCRIBE_FALLBACK: i64 = 50359;
const NOTIMESTAMPS_FALLBACK: i64 = 50363;
const MAX_TOKENS: usize = 224;
const PARTIAL_MAX_TOKENS: usize = 10;
const MIN_FINAL_TOKENS: usize = 24;
const REPEAT_TOKEN_BREAK_THRESHOLD: usize = 14;
const NO_REPEAT_NGRAM_SIZE: usize = 0;
const MAX_TOKEN_TAIL_HISTORY: usize = 64;
const MAX_TAIL_TOKEN_OCCURRENCES: usize = 14;
const TOKEN_REPEAT_PENALTY: f32 = 0.14;
const PHRASE_BIAS_LOGIT_BOOST: f32 = 0.45;
const TOKENS_PER_SECOND_ESTIMATE: f32 = 6.8;
const DECODE_TOKEN_OVERHEAD: usize = 12;

// ── Model config ─────────────────────────────────────────────────────────────

pub struct OnnxModelConfig {
    pub encoder_path: PathBuf,
    pub decoder_path: PathBuf,
    pub decoder_with_past_path: Option<PathBuf>,
    pub tokenizer_path: PathBuf,
}

impl Default for OnnxModelConfig {
    fn default() -> Self {
        let dir = selected_models_dir();
        let decoder_with_past = dir.join("decoder_with_past_model.onnx");
        Self {
            encoder_path: dir.join("encoder_model.onnx"),
            decoder_path: dir.join("decoder_model.onnx"),
            decoder_with_past_path: decoder_with_past.exists().then_some(decoder_with_past),
            tokenizer_path: dir.join("tokenizer.json"),
        }
    }
}

fn selected_models_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("DICTUM_MODEL_DIR") {
        let p = PathBuf::from(explicit.trim());
        if !explicit.trim().is_empty() {
            return p;
        }
    }
    let default_dir = default_models_dir();
    if let Ok(profile) = std::env::var("DICTUM_MODEL_PROFILE") {
        let profile = profile.trim();
        if !profile.is_empty() && !profile.eq_ignore_ascii_case("small") {
            let profile_dir = default_dir.join(profile);
            if has_required_whisper_files(&profile_dir) {
                return profile_dir;
            }
            warn!(
                profile,
                profile_dir = ?profile_dir,
                fallback_dir = ?default_dir,
                "requested DICTUM_MODEL_PROFILE is missing required files; falling back to default models dir"
            );
        }
    }
    default_dir
}

fn has_required_whisper_files(dir: &Path) -> bool {
    dir.join("encoder_model.onnx").exists()
        && dir.join("decoder_model.onnx").exists()
        && dir.join("tokenizer.json").exists()
}

pub fn default_models_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(|p| {
                PathBuf::from(p)
                    .join("Lattice Labs")
                    .join("Dictum")
                    .join("models")
            })
            .unwrap_or_else(|| PathBuf::from("models"))
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
            .join("models")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrtExecutionPreference {
    Auto,
    Cpu,
    DirectML,
}

fn ort_execution_preference() -> OrtExecutionPreference {
    match std::env::var("DICTUM_ORT_EP")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "cpu" => OrtExecutionPreference::Cpu,
        "dml" | "directml" => OrtExecutionPreference::DirectML,
        _ => OrtExecutionPreference::Auto,
    }
}

fn create_session(model_path: &Path) -> Result<Session> {
    let pref = ort_execution_preference();
    let logical_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let default_intra = if pref == OrtExecutionPreference::DirectML {
        logical_cores.clamp(4, 12)
    } else {
        logical_cores.clamp(2, 12)
    };
    let default_inter = if pref == OrtExecutionPreference::DirectML {
        2usize
    } else {
        1usize
    };
    let intra_threads = std::env::var("DICTUM_ORT_INTRA_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default_intra)
        .clamp(1, 32);
    let inter_threads = std::env::var("DICTUM_ORT_INTER_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default_inter)
        .clamp(1, 8);
    let parallel_execution = std::env::var("DICTUM_ORT_PARALLEL")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(pref == OrtExecutionPreference::DirectML);

    let mut builder = SessionBuilder::new()
        .map_err(|e| DictumError::OnnxSession(e.to_string()))?
        .with_intra_threads(intra_threads)
        .map_err(|e| DictumError::OnnxSession(e.to_string()))?
        .with_inter_threads(inter_threads)
        .map_err(|e| DictumError::OnnxSession(e.to_string()))?
        .with_parallel_execution(parallel_execution)
        .map_err(|e| DictumError::OnnxSession(e.to_string()))?
        .with_optimization_level(GraphOptimizationLevel::All)
        .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
    info!(
        intra_threads,
        inter_threads, parallel_execution, logical_cores, "ONNX session threading configured"
    );

    #[cfg(target_os = "windows")]
    {
        builder = match pref {
            OrtExecutionPreference::Cpu => {
                info!("ONNX EP preference=cpu");
                builder
                    .with_execution_providers([ep::CPU::default().build()])
                    .map_err(|e| DictumError::OnnxSession(e.to_string()))?
            }
            OrtExecutionPreference::DirectML => {
                info!("ONNX EP preference=directml (strict)");
                builder
                    .with_execution_providers([
                        ep::DirectML::default()
                            .with_device_id(0)
                            .build()
                            .error_on_failure(),
                        ep::CPU::default().build(),
                    ])
                    .map_err(|e| DictumError::OnnxSession(e.to_string()))?
            }
            OrtExecutionPreference::Auto => {
                info!("ONNX EP preference=auto (directml -> cpu)");
                builder
                    .with_execution_providers([
                        ep::DirectML::default()
                            .with_device_id(0)
                            .build()
                            .fail_silently(),
                        ep::CPU::default().build(),
                    ])
                    .map_err(|e| DictumError::OnnxSession(e.to_string()))?
            }
        };
    }

    #[cfg(not(target_os = "windows"))]
    {
        if pref == OrtExecutionPreference::DirectML {
            warn!("DICTUM_ORT_EP=directml requested on non-Windows host; using CPU EP");
        }
        builder = builder
            .with_execution_providers([ep::CPU::default().build()])
            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
    }

    builder
        .commit_from_file(model_path)
        .map_err(|e| DictumError::OnnxSession(e.to_string()))
}

// ── OnnxModel ────────────────────────────────────────────────────────────────

pub struct OnnxModel {
    config: OnnxModelConfig,
    encoder: Option<Session>,
    decoder: Option<Session>,
    decoder_with_past: Option<Session>,
    tokenizer: Option<Tokenizer>,
    n_mels: usize,
    mel_filters: Vec<Vec<f32>>,
    hann_window: Vec<f32>,
    fft: Arc<dyn rustfft::Fft<f32>>,
    utterance_count: u64,
}

impl OnnxModel {
    pub fn new(config: OnnxModelConfig) -> Self {
        let hann_window = build_hann_window(N_FFT);
        let mel_filters = build_mel_filters(FFT_SIZE, 16_000, N_MELS, 0.0, 8_000.0);
        let fft = Arc::from(FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE));

        Self {
            config,
            encoder: None,
            decoder: None,
            decoder_with_past: None,
            tokenizer: None,
            n_mels: N_MELS,
            mel_filters,
            hann_window,
            fft,
            utterance_count: 0,
        }
    }

    fn log_mel_spectrogram(&self, samples: &[f32], active_samples: usize) -> Array3<f32> {
        let mut normalized = samples.to_vec();
        normalize_rms_in_place(&mut normalized, 0.10);
        let centered = reflect_pad(&normalized, N_FFT / 2);
        let active_samples = active_samples.min(MEL_SAMPLES);
        let active_frames = ((active_samples + N_FFT + HOP - 1) / HOP).clamp(1, N_FRAMES);

        let mut mel = Array3::<f32>::zeros((1, self.n_mels, N_FRAMES));
        let mut fft_buf = vec![Complex::new(0.0f32, 0.0); FFT_SIZE];

        // Most utterances are far shorter than 30s. Skip FFT work for guaranteed
        // zero-padded tail frames to reduce frontend CPU time.
        for frame in 0..active_frames {
            let start = frame * HOP;

            for v in fft_buf.iter_mut() {
                *v = Complex::new(0.0, 0.0);
            }
            for i in 0..N_FFT {
                let s = centered[start + i];
                fft_buf[i] = Complex::new(s * self.hann_window[i], 0.0);
            }
            self.fft.process(&mut fft_buf);

            for m in 0..self.n_mels {
                let mut energy = 0.0f32;
                for k in 0..N_FREQS {
                    energy += self.mel_filters[m][k] * fft_buf[k].norm_sqr();
                }
                mel[[0, m, frame]] = energy;
            }
        }

        mel.mapv_inplace(|v| v.max(1e-10).log10());
        let max_val = mel.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        mel.mapv_inplace(|v| v.max(max_val - 8.0));
        mel.mapv_inplace(|v| (v + 4.0) / 4.0);
        mel
    }

    /// Greedy decode, returning the full token sequence including SOT prefix.
    ///
    fn greedy_decode(
        decoder: &mut Session,
        mut decoder_with_past: Option<&mut Session>,
        tokenizer: &Tokenizer,
        enc_data: &[f32],
        enc_n_frames: usize,
        enc_d_model: usize,
        max_decode_steps: usize,
        prefix: &[i64],
        eot_id: i64,
        timestamp_begin: Option<i64>,
        begin_suppress_tokens: &[i64],
        always_suppress_tokens: &[i64],
        phrase_bias_token_ids: &HashSet<i64>,
        partial: bool,
    ) -> Result<Vec<i64>> {
        let max_steps = max_decode_steps.clamp(1, MAX_TOKENS);
        let min_decode_steps_before_eot = if partial { 1 } else { 2 };
        let debug_mode = is_debug_transcribe();
        let mut tokens: Vec<i64> = prefix.to_vec();
        let mut repeated_token_count = 0usize;
        let with_past_input_names = decoder_with_past
            .as_ref()
            .map(|s| decoder_with_past_input_names(s))
            .unwrap_or_default();
        let with_past_required: HashSet<String> = with_past_input_names.iter().cloned().collect();
        let mut past_values: HashMap<String, DynValue> = HashMap::new();

        if debug_mode {
            info!(
                prefix = ?prefix,
                partial,
                max_steps,
                "DICTUM_DEBUG_TRANSCRIBE: starting decode"
            );
        }

        for step in 0..max_steps {
            let seq = tokens.len();
            let banned_no_repeat = if partial {
                HashSet::new()
            } else {
                HashSet::from_iter(banned_next_tokens_no_repeat_ngram(
                    &tokens,
                    prefix.len(),
                    NO_REPEAT_NGRAM_SIZE,
                ))
            };
            let generated = tokens.get(prefix.len()..).unwrap_or(&[]);
            let mut tail_counts: HashMap<i64, usize> = HashMap::new();
            for &tok in generated
                .iter()
                .rev()
                .take(MAX_TOKEN_TAIL_HISTORY.min(generated.len()))
            {
                *tail_counts.entry(tok).or_insert(0) += 1;
            }

            let mut dec_out = if step > 0
                && !with_past_input_names.is_empty()
                && with_past_input_names
                    .iter()
                    .all(|name| past_values.contains_key(name))
            {
                let last_token = [*tokens.last().unwrap_or(&eot_id)];
                let with_past_out: Result<SessionOutputs<'_>> = {
                    let input_ids = TensorRef::from_array_view(([1_i64, 1_i64], &last_token[..]))
                        .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                    let encoder_hidden_states = TensorRef::from_array_view((
                        [1_i64, enc_n_frames as i64, enc_d_model as i64],
                        enc_data,
                    ))
                    .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                    let mut inputs: Vec<(String, SessionInputValue<'_>)> =
                        Vec::with_capacity(2 + with_past_input_names.len());
                    inputs.push(("input_ids".into(), SessionInputValue::from(input_ids)));
                    inputs.push((
                        "encoder_hidden_states".into(),
                        SessionInputValue::from(encoder_hidden_states),
                    ));
                    for name in &with_past_input_names {
                        let Some(v) = past_values.get(name) else {
                            return Err(DictumError::OnnxSession(format!(
                                "missing cached past key/value input: {name}"
                            )));
                        };
                        inputs.push((name.clone(), SessionInputValue::from(v)));
                    }
                    let Some(decoder_with_past) = decoder_with_past.as_deref_mut() else {
                        return Err(DictumError::OnnxSession(
                            "decoder_with_past session unavailable".into(),
                        ));
                    };
                    decoder_with_past
                        .run(inputs)
                        .map_err(|e| DictumError::OnnxSession(e.to_string()))
                };

                match with_past_out {
                    Ok(out) => out,
                    Err(e) => {
                        debug!(error = %e, step, "decoder_with_past step failed; falling back");
                        let input_ids =
                            TensorRef::from_array_view(([1_i64, seq as i64], tokens.as_slice()))
                                .map_err(|err| DictumError::OnnxSession(err.to_string()))?;
                        let encoder_hidden_states = TensorRef::from_array_view((
                            [1_i64, enc_n_frames as i64, enc_d_model as i64],
                            enc_data,
                        ))
                        .map_err(|err| DictumError::OnnxSession(err.to_string()))?;
                        decoder
                            .run(ort::inputs![
                                "input_ids"             => input_ids,
                                "encoder_hidden_states" => encoder_hidden_states,
                            ])
                            .map_err(|err| DictumError::OnnxSession(err.to_string()))?
                    }
                }
            } else {
                // Zero-copy views over decoder inputs to avoid per-token tensor copies.
                let input_ids =
                    TensorRef::from_array_view(([1_i64, seq as i64], tokens.as_slice()))
                        .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                let encoder_hidden_states = TensorRef::from_array_view((
                    [1_i64, enc_n_frames as i64, enc_d_model as i64],
                    enc_data,
                ))
                .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                decoder
                    .run(ort::inputs![
                        "input_ids"             => input_ids,
                        "encoder_hidden_states" => encoder_hidden_states,
                    ])
                    .map_err(|e| DictumError::OnnxSession(e.to_string()))?
            };

            if !with_past_required.is_empty() {
                let collected = collect_present_key_values(&mut dec_out, &with_past_required);
                if !collected.is_empty() {
                    for (name, value) in collected {
                        past_values.insert(name, value);
                    }
                }
            }

            let (_, logit_data) = dec_out["logits"]
                .try_extract_tensor::<f32>()
                .map_err(|e| DictumError::OnnxSession(e.to_string()))?;

            // logit_data is flat [1 * seq * vocab]; extract last-token slice
            let vocab_size = logit_data.len() / seq;
            let start = (seq - 1) * vocab_size;
            let last_row = &logit_data[start..start + vocab_size];

            let (next, _next_logit) = last_row
                .iter()
                .enumerate()
                .fold(
                    (None::<(usize, f32)>, None::<(usize, f32)>),
                    |(best_non_ts, best_any), (i, &v)| {
                        let token_id = i as i64;
                        let tail_count = tail_counts.get(&token_id).copied().unwrap_or(0);
                        let phrase_bias = if phrase_bias_token_ids.contains(&token_id) {
                            PHRASE_BIAS_LOGIT_BOOST
                        } else {
                            0.0
                        };
                        let penalized = v + phrase_bias - TOKEN_REPEAT_PENALTY * tail_count as f32;
                        let suppressed_for_begin =
                            step == 0 && begin_suppress_tokens.contains(&token_id);
                        let suppressed_always = always_suppress_tokens.contains(&token_id);
                        let suppressed_early_eot =
                            token_id == eot_id && step < min_decode_steps_before_eot;
                        let suppressed_no_repeat = banned_no_repeat.contains(&token_id);
                        let suppressed_tail_repetition = !partial
                            && tail_count >= MAX_TAIL_TOKEN_OCCURRENCES
                            && token_id != eot_id;
                        let next_best_any = match best_any {
                            Some((_, b)) if b >= penalized => best_any,
                            _ => Some((i, penalized)),
                        };
                        let is_ts = timestamp_begin.map(|tb| (i as i64) >= tb).unwrap_or(false);
                        let next_best_non_ts = if is_ts
                            || suppressed_for_begin
                            || suppressed_always
                            || suppressed_early_eot
                            || suppressed_no_repeat
                            || suppressed_tail_repetition
                        {
                            best_non_ts
                        } else {
                            match best_non_ts {
                                Some((_, b)) if b >= penalized => best_non_ts,
                                _ => Some((i, penalized)),
                            }
                        };
                        (next_best_non_ts, next_best_any)
                    },
                )
                .0
                .or_else(|| {
                    last_row
                        .iter()
                        .enumerate()
                        .max_by(|(_, a): &(usize, &f32), (_, b)| {
                            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|(i, v)| (i, *v))
                })
                .map(|(i, score)| (i as i64, score))
                .unwrap_or((eot_id, f32::NEG_INFINITY));

            tokens.push(next);
            if tokens.len() >= 2 && tokens[tokens.len() - 2] == next {
                repeated_token_count = repeated_token_count.saturating_add(1);
            } else {
                repeated_token_count = 0;
            }

            if debug_mode && step < 20 {
                let token_text = tokenizer
                    .decode(&[next as u32], true)
                    .unwrap_or_else(|_| format!("<token:{}>", next));
                info!(
                    step,
                    token_id = next,
                    token_text = %token_text,
                    "DICTUM_DEBUG_TRANSCRIBE: token"
                );
            }

            if next == eot_id {
                if debug_mode {
                    info!("DICTUM_DEBUG_TRANSCRIBE: EOT reached at step {}", step);
                }
                break;
            }
            if repeated_token_count >= REPEAT_TOKEN_BREAK_THRESHOLD {
                debug!(
                    repeated_token_count,
                    token = next,
                    "breaking decode early due to repeated-token loop"
                );
                break;
            }
            let generated = tokens.get(prefix.len()..).unwrap_or(&[]);
            if has_repeating_tail_pattern(generated) {
                debug!(
                    step,
                    generated_tokens = generated.len(),
                    "breaking decode early due to repeating tail pattern"
                );
                break;
            }
        }

        Ok(tokens)
    }

    fn token_id_or(tokenizer: &Tokenizer, token: &str, fallback: i64) -> i64 {
        tokenizer
            .token_to_id(token)
            .map(|id| id as i64)
            .unwrap_or(fallback)
    }

    fn decode_prefix_candidates(
        tokenizer: &Tokenizer,
        language_hint: DecodeLanguageHint,
    ) -> Vec<Vec<i64>> {
        let sot = Self::token_id_or(tokenizer, "<|startoftranscript|>", SOT_FALLBACK);
        let en = Self::token_id_or(tokenizer, "<|en|>", ENGLISH_FALLBACK);
        let transcribe = Self::token_id_or(tokenizer, "<|transcribe|>", TRANSCRIBE_FALLBACK);
        let notimestamps = Self::token_id_or(tokenizer, "<|notimestamps|>", NOTIMESTAMPS_FALLBACK);
        let zh = tokenizer.token_to_id("<|zh|>").map(|id| id as i64);
        let ru = tokenizer.token_to_id("<|ru|>").map(|id| id as i64);

        let mut out: Vec<Vec<i64>> = Vec::new();
        let mut push_prefix = |prefix: Vec<i64>| {
            if !out.contains(&prefix) {
                out.push(prefix);
            }
        };

        match language_hint {
            DecodeLanguageHint::English => {
                push_prefix(vec![sot, en, transcribe, notimestamps]);
                push_prefix(vec![sot, en, transcribe]);
            }
            DecodeLanguageHint::Mandarin => {
                if let Some(zh) = zh {
                    push_prefix(vec![sot, zh, transcribe, notimestamps]);
                    push_prefix(vec![sot, zh, transcribe]);
                }
            }
            DecodeLanguageHint::Russian => {
                if let Some(ru) = ru {
                    push_prefix(vec![sot, ru, transcribe, notimestamps]);
                    push_prefix(vec![sot, ru, transcribe]);
                }
            }
            DecodeLanguageHint::Auto => {}
        }

        // Auto-detect first so multilingual dictation (English + Mandarin + Russian)
        // doesn't get forced through an English token.
        push_prefix(vec![sot, transcribe, notimestamps]);
        push_prefix(vec![sot, transcribe]);
        if let Some(zh) = zh {
            push_prefix(vec![sot, zh, transcribe, notimestamps]);
        }
        if let Some(ru) = ru {
            push_prefix(vec![sot, ru, transcribe, notimestamps]);
        }
        // Keep English fallback late in the stack for slang-heavy English inputs.
        push_prefix(vec![sot, en, transcribe, notimestamps]);
        push_prefix(vec![sot, en, transcribe]);
        out
    }
}

impl SpeechModel for OnnxModel {
    fn warm_up(&mut self) -> Result<()> {
        info!("=== Dictum ONNX Model Startup Report ===");

        for path in [
            &self.config.encoder_path,
            &self.config.decoder_path,
            &self.config.tokenizer_path,
        ] {
            if !path.exists() {
                info!("  {:?}: NOT FOUND", path);
                return Err(DictumError::ModelNotFound { path: path.clone() });
            }
            let metadata = std::fs::metadata(path);
            let size_mb = metadata
                .map(|m| m.len() as f64 / 1_048_576.0)
                .unwrap_or(0.0);
            info!("  {:?}: {:.2} MB", path, size_mb);
        }

        info!(
            "loading OnnxModel encoder from {:?}",
            self.config.encoder_path
        );
        self.encoder = Some(create_session(&self.config.encoder_path)?);

        let encoder = self.encoder.as_ref().unwrap();
        info!("  encoder inputs:");
        for input in encoder.inputs().iter() {
            info!("    {}", input.name());
        }
        info!("  encoder outputs:");
        for output in encoder.outputs().iter() {
            info!("    {}", output.name());
        }
        if let Some(shape) = encoder
            .inputs()
            .first()
            .and_then(|i| i.dtype().tensor_shape())
            .filter(|s| s.len() >= 2)
        {
            let mel_bins = shape[1];
            if mel_bins > 0 {
                let mel_bins = mel_bins as usize;
                if mel_bins != self.n_mels {
                    info!(
                        previous = self.n_mels,
                        detected = mel_bins,
                        "detected encoder mel-bin dimension; updating frontend"
                    );
                    self.n_mels = mel_bins;
                    self.mel_filters =
                        build_mel_filters(FFT_SIZE, 16_000, self.n_mels, 0.0, 8_000.0);
                }
            }
        }

        info!(
            "loading OnnxModel decoder from {:?}",
            self.config.decoder_path
        );
        self.decoder = Some(create_session(&self.config.decoder_path)?);

        let decoder = self.decoder.as_ref().unwrap();
        info!("  decoder inputs:");
        for input in decoder.inputs().iter() {
            info!("    {}", input.name());
        }
        info!("  decoder outputs:");
        for output in decoder.outputs().iter() {
            info!("    {}", output.name());
        }

        if let Some(path) = self
            .config
            .decoder_with_past_path
            .as_ref()
            .filter(|p| p.exists())
        {
            info!("loading OnnxModel decoder_with_past from {:?}", path);
            self.decoder_with_past = Some(create_session(path)?);
            let decoder_with_past = self.decoder_with_past.as_ref().unwrap();
            info!("  decoder_with_past inputs:");
            for input in decoder_with_past.inputs().iter() {
                info!("    {}", input.name());
            }
            info!("  decoder_with_past outputs:");
            for output in decoder_with_past.outputs().iter() {
                info!("    {}", output.name());
            }
        } else {
            self.decoder_with_past = None;
            info!("decoder_with_past_model.onnx not found; using baseline decoder path");
        }

        info!("loading tokenizer from {:?}", self.config.tokenizer_path);
        self.tokenizer = Some(
            Tokenizer::from_file(&self.config.tokenizer_path)
                .map_err(|e| DictumError::OnnxSession(e.to_string()))?,
        );

        let tokenizer = self.tokenizer.as_ref().unwrap();
        info!("  tokenizer vocab size: {}", tokenizer.get_vocab_size(true));

        // Dummy encoder forward pass to populate CPU caches.
        // Array3<f32> has Ix3: Dimension + 'static → OwnedTensorArrayData satisfied.
        let dummy = Array3::<f32>::zeros((1, self.n_mels, N_FRAMES));
        let dummy_val = Value::from_array(dummy)
            .map_err(|e: ort::Error| DictumError::OnnxSession(e.to_string()))?;
        let enc = self.encoder.as_mut().unwrap();
        enc.run(ort::inputs!["input_features" => dummy_val])
            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;

        info!("=== OnnxModel warm-up complete ===");
        Ok(())
    }

    fn transcribe(&mut self, chunk: &AudioChunk, partial: bool) -> Result<Vec<TranscriptSegment>> {
        // Verify models are loaded before taking mutable borrows.
        if self.encoder.is_none() || self.decoder.is_none() || self.tokenizer.is_none() {
            return Err(DictumError::OnnxSession(
                "model not loaded — call warm_up()".into(),
            ));
        }

        // 1. Pad / trim to 30 s.
        let mut samples = chunk.samples.clone();
        let active_samples = samples.len().min(MEL_SAMPLES);
        samples.resize(MEL_SAMPLES, 0.0);

        // 2. Log-mel spectrogram (before taking mutable session borrows).
        let mel = self.log_mel_spectrogram(&samples, active_samples);
        let mel_val = Value::from_array(mel)
            .map_err(|e: ort::Error| DictumError::OnnxSession(e.to_string()))?;

        // SAFETY: checked is_some() above.
        let encoder = self.encoder.as_mut().unwrap();
        let decoder = self.decoder.as_mut().unwrap();
        let mut decoder_with_past = self.decoder_with_past.as_mut();
        let tokenizer = self.tokenizer.as_ref().unwrap();

        // 3. Encoder.
        let enc_out = encoder
            .run(ort::inputs!["input_features" => mel_val])
            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
        let (enc_shape_raw, enc_data) = enc_out["last_hidden_state"]
            .try_extract_tensor::<f32>()
            .map_err(|e| DictumError::OnnxSession(e.to_string()))?;

        // Derive encoder time and feature dims from shape.
        // enc_shape is [1, n_enc_frames, d_model]; use data length as fallback.
        let (enc_n_frames, enc_d_model) = {
            let total = enc_data.len(); // 1 * n * d
                                        // Try to read from ort Shape (treats elements as i64 via Deref)
            if enc_shape_raw.len() >= 3 {
                let n = enc_shape_raw[1] as usize;
                let d = enc_shape_raw[2] as usize;
                (n, d)
            } else {
                // Fallback: assume standard Whisper small (d_model=384)
                let d = 384usize;
                (total / d, d)
            }
        };
        // 4. Greedy decode. Try a few Whisper-compatible prefixes so tokenizer
        // variants still decode correctly.
        let eot_id = tokenizer
            .token_to_id("<|endoftext|>")
            .map(|id| id as i64)
            .unwrap_or(EOT);
        let decode_prefixes = Self::decode_prefix_candidates(tokenizer, decode_language_hint());
        let timestamp_begin = tokenizer.token_to_id("<|0.00|>").map(|id| id as i64);
        let mut begin_suppress_tokens = vec![220i64, eot_id];
        begin_suppress_tokens.sort_unstable();
        begin_suppress_tokens.dedup();
        let mut always_suppress_tokens = vec![];
        for tok in [
            "<|startoftranscript|>",
            "<|translate|>",
            "<|transcribe|>",
            "<|notimestamps|>",
            "<|nospeech|>",
        ] {
            if let Some(id) = tokenizer.token_to_id(tok) {
                always_suppress_tokens.push(id as i64);
            }
        }
        always_suppress_tokens.sort_unstable();
        always_suppress_tokens.dedup();

        let decode_to_text =
            |tokens: &[i64], prefix_len: usize| -> Result<(String, Option<&'static str>)> {
                let text_tokens: Vec<u32> = tokens[prefix_len..]
                    .iter()
                    .take_while(|&&t| t != eot_id)
                    .filter(|&&t| timestamp_begin.map(|tb| t < tb).unwrap_or(true))
                    .map(|&t| t as u32)
                    .collect();
                if text_tokens.is_empty() {
                    return Ok((String::new(), Some("no_tokens_before_eot")));
                }
                let decoded = tokenizer
                    .decode(&text_tokens, true)
                    .map_err(|e| DictumError::OnnxSession(e.to_string()))?;
                let text = decoded.trim().to_string();
                if text.is_empty() {
                    return Ok((String::new(), Some("decoded_empty_text")));
                }
                Ok((text, None))
            };
        let phrase_bias_terms = phrase_bias_terms_from_env();
        let phrase_bias_token_ids = phrase_bias_token_ids(tokenizer, &phrase_bias_terms);

        let debug_mode = is_debug_transcribe();
        let mut tokens = Vec::new();
        let mut text = String::new();
        let mut empty_reason = Some("decode_not_attempted");
        let audio_seconds = active_samples as f32 / chunk.sample_rate.max(1) as f32;
        let adaptive_final_steps = {
            // Heuristic: estimated tokens/s + prompt allowance. Clamp to avoid runaway
            // decoding when EOT is not emitted promptly.
            let estimated = ((active_samples as f32 / chunk.sample_rate as f32)
                * TOKENS_PER_SECOND_ESTIMATE)
                .ceil() as usize
                + DECODE_TOKEN_OVERHEAD;
            estimated.clamp(MIN_FINAL_TOKENS, MAX_TOKENS)
        };
        let max_decode_steps = if partial {
            PARTIAL_MAX_TOKENS.min(adaptive_final_steps)
        } else {
            adaptive_final_steps
        };
        let fast_decode_steps = if partial {
            max_decode_steps
        } else {
            let short_cap = if audio_seconds <= 4.0 {
                96
            } else if audio_seconds <= 8.0 {
                128
            } else {
                160
            };
            max_decode_steps.clamp(MIN_FINAL_TOKENS, short_cap)
        };

        let mut try_prefix =
            |prefix: &[i64], decode_steps: usize| -> Result<(Option<String>, bool)> {
                if debug_mode {
                    info!(
                        prefix = ?prefix, decode_steps,
                        "DICTUM_DEBUG_TRANSCRIBE: trying decode prefix"
                    );
                }
                let candidate_tokens = Self::greedy_decode(
                    decoder,
                    decoder_with_past.as_mut().map(|s| &mut **s),
                    tokenizer,
                    enc_data,
                    enc_n_frames,
                    enc_d_model,
                    decode_steps,
                    prefix,
                    eot_id,
                    timestamp_begin,
                    &begin_suppress_tokens,
                    &always_suppress_tokens,
                    &phrase_bias_token_ids,
                    partial,
                )?;
                let generated_len = candidate_tokens.len().saturating_sub(prefix.len());
                let ended_with_eot = candidate_tokens.last().copied() == Some(eot_id);
                let reached_ceiling_no_eot = generated_len >= decode_steps && !ended_with_eot;
                let (candidate_text_raw, candidate_reason) =
                    decode_to_text(&candidate_tokens, prefix.len())?;
                tokens = candidate_tokens;
                empty_reason = candidate_reason;
                let candidate_text = postprocess_transcript_text(&candidate_text_raw);
                if candidate_text.is_empty() {
                    return Ok((None, reached_ceiling_no_eot));
                }
                if !partial && is_low_quality_transcript_text(&candidate_text, audio_seconds) {
                    warn!(
                        text_len = candidate_text.len(),
                        audio_seconds = format_args!("{audio_seconds:.2}"),
                        "dropping low-quality transcript candidate"
                    );
                    empty_reason = Some("low_quality_candidate_filtered");
                    return Ok((None, reached_ceiling_no_eot));
                }
                Ok((Some(candidate_text), reached_ceiling_no_eot))
            };

        let mut ceiling_retry_needed = false;
        let fast_prefix_limit = 1usize;
        for prefix in decode_prefixes.iter().take(fast_prefix_limit) {
            let (candidate, reached_ceiling_no_eot) = try_prefix(prefix, fast_decode_steps)?;
            if let Some(candidate_text) = candidate {
                text = candidate_text;
                if !partial
                    && reached_ceiling_no_eot
                    && likely_truncated_transcript(&text, audio_seconds)
                {
                    ceiling_retry_needed = true;
                }
                break;
            }
        }
        if text.is_empty() && decode_prefixes.len() > fast_prefix_limit {
            for prefix in decode_prefixes.iter().skip(fast_prefix_limit) {
                let (candidate, reached_ceiling_no_eot) = try_prefix(prefix, fast_decode_steps)?;
                if let Some(candidate_text) = candidate {
                    text = candidate_text;
                    if !partial
                        && reached_ceiling_no_eot
                        && likely_truncated_transcript(&text, audio_seconds)
                    {
                        ceiling_retry_needed = true;
                    }
                    break;
                }
            }
        }

        if !partial
            && max_decode_steps > fast_decode_steps
            && (text.is_empty() || ceiling_retry_needed)
        {
            let mut retry_text = String::new();
            for prefix in decode_prefixes.iter().take(fast_prefix_limit) {
                let (candidate, _) = try_prefix(prefix, max_decode_steps)?;
                if let Some(candidate_text) = candidate {
                    retry_text = candidate_text;
                    break;
                }
            }
            if retry_text.is_empty() && decode_prefixes.len() > fast_prefix_limit {
                for prefix in decode_prefixes.iter().skip(fast_prefix_limit) {
                    let (candidate, _) = try_prefix(prefix, max_decode_steps)?;
                    if let Some(candidate_text) = candidate {
                        retry_text = candidate_text;
                        break;
                    }
                }
            }
            if !retry_text.is_empty() {
                text = retry_text;
            }
        }

        if !partial
            && post_utterance_refinement_enabled()
            && !text.is_empty()
            && audio_seconds >= 5.0
        {
            let refine_decode_steps = max_decode_steps
                .saturating_add(48)
                .clamp(MIN_FINAL_TOKENS, MAX_TOKENS);
            let mut best_refine_text = text.clone();
            let mut best_score = transcript_quality_score(&best_refine_text, audio_seconds);
            let best_words = best_refine_text.split_whitespace().count().max(1);
            for prefix in &decode_prefixes {
                let (candidate, _) = try_prefix(prefix, refine_decode_steps)?;
                if let Some(candidate_text) = candidate {
                    if is_low_quality_transcript_text(&candidate_text, audio_seconds) {
                        continue;
                    }
                    let candidate_words = candidate_text.split_whitespace().count();
                    if audio_seconds <= 8.0 && candidate_words > best_words.saturating_mul(2) {
                        continue;
                    }
                    let candidate_score = transcript_quality_score(&candidate_text, audio_seconds);
                    if candidate_score > best_score + 0.7 {
                        best_refine_text = candidate_text;
                        best_score = candidate_score;
                    }
                }
            }
            text = best_refine_text;
        }

        if text.is_empty() && !partial {
            if let Some(fallback_text) =
                openai_cloud_fallback_text(&chunk.samples, chunk.sample_rate)
            {
                let fallback_text = postprocess_transcript_text(&fallback_text);
                if fallback_text.is_empty() {
                    empty_reason = Some("cloud_fallback_empty_after_postprocess");
                } else {
                    text = fallback_text;
                    empty_reason = None;
                    info!("onnx empty decode recovered by OpenAI cloud fallback");
                }
            }
        }

        if text.is_empty() && !partial {
            if let Some(fallback_text) =
                windows_dictation_fallback_text(&chunk.samples, chunk.sample_rate)
            {
                let fallback_text = postprocess_transcript_text(&fallback_text);
                if fallback_text.is_empty() {
                    empty_reason = Some("fallback_empty_after_postprocess");
                } else {
                    text = fallback_text;
                    empty_reason = None;
                    info!("onnx empty decode recovered by Windows dictation fallback");
                }
            }
        }

        if text.is_empty() {
            debug!(?tokens, ?empty_reason, "onnx decode produced empty text");
            if debug_mode {
                info!(
                    reason = ?empty_reason,
                    "DICTUM_DEBUG_TRANSCRIBE: all decode paths produced empty text"
                );
            }
            return Ok(vec![]);
        }

        self.utterance_count += 1;
        let kind = if partial {
            SegmentKind::Partial
        } else {
            SegmentKind::Final
        };

        Ok(vec![TranscriptSegment {
            id: self.utterance_count.to_string(),
            text: text.clone(),
            kind,
            confidence: estimate_segment_confidence(&text, audio_seconds, partial),
        }])
    }

    fn reset(&mut self) {}
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn decoder_with_past_input_names(session: &Session) -> Vec<String> {
    session
        .inputs()
        .iter()
        .map(|o| o.name().to_string())
        .filter(|n| n.starts_with("past_key_values."))
        .collect()
}

fn present_to_past_input_name(output_name: &str) -> Option<String> {
    if let Some(rest) = output_name.strip_prefix("present.") {
        return Some(format!("past_key_values.{rest}"));
    }
    if let Some(rest) = output_name.strip_prefix("present_key_values.") {
        return Some(format!("past_key_values.{rest}"));
    }
    if output_name.starts_with("past_key_values.") {
        return Some(output_name.to_string());
    }
    None
}

fn collect_present_key_values(
    outputs: &mut SessionOutputs<'_>,
    required_inputs: &HashSet<String>,
) -> HashMap<String, DynValue> {
    let mut out: HashMap<String, DynValue> = HashMap::new();
    let names: Vec<String> = outputs.keys().map(|n| n.to_string()).collect();
    for name in names {
        let Some(mapped) = present_to_past_input_name(&name) else {
            continue;
        };
        if !required_inputs.contains(&mapped) {
            continue;
        }
        if let Some(value) = outputs.remove(&name) {
            out.insert(mapped, value);
        }
    }
    out
}

fn banned_next_tokens_no_repeat_ngram(
    tokens: &[i64],
    prefix_len: usize,
    ngram_size: usize,
) -> Vec<i64> {
    if ngram_size < 2 {
        return vec![];
    }
    let Some(generated) = tokens.get(prefix_len..) else {
        return vec![];
    };
    if generated.len() + 1 < ngram_size {
        return vec![];
    }

    let context = &generated[generated.len() - (ngram_size - 1)..];
    let mut banned = Vec::new();
    for w in generated.windows(ngram_size) {
        if &w[..ngram_size - 1] == context {
            let next = w[ngram_size - 1];
            if !banned.contains(&next) {
                banned.push(next);
            }
        }
    }
    banned
}

fn has_repeating_tail_pattern(generated: &[i64]) -> bool {
    // Detect repeated n-gram loops at the tail, e.g. [a,b,a,b,a,b] or [x,x,x].
    let len = generated.len();
    for n in 1..=8 {
        if len < n * 3 {
            continue;
        }
        let a = &generated[len - n..len];
        let b = &generated[len - 2 * n..len - n];
        let c = &generated[len - 3 * n..len - 2 * n];
        if a == b && b == c {
            return true;
        }
    }
    false
}

fn postprocess_transcript_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Normalize whitespace and remove spaces before punctuation.
    let mut compact = String::with_capacity(trimmed.len() + 8);
    let mut prev_was_space = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !prev_was_space {
                compact.push(' ');
            }
            prev_was_space = true;
            continue;
        }
        if matches!(ch, '.' | ',' | '!' | '?' | ';' | ':') && compact.ends_with(' ') {
            compact.pop();
        }
        compact.push(ch);
        prev_was_space = false;
    }
    let mut out = compact.trim().to_string();

    // Remove leading punctuation artifacts from decoder restarts.
    out = out
        .trim_start_matches(|ch: char| matches!(ch, ',' | ';' | ':' | '.' | '!' | '?'))
        .trim_start()
        .to_string();

    // Uppercase standalone "i" pronoun.
    out = out
        .split_whitespace()
        .map(|w| if w == "i" { "I" } else { w })
        .collect::<Vec<_>>()
        .join(" ");

    // Capitalize sentence starts.
    out = capitalize_sentence_starts(&out);

    // Add terminal punctuation for longer phrases lacking it.
    let has_terminal_punct = out.ends_with('.') || out.ends_with('!') || out.ends_with('?');
    let word_count = out.split_whitespace().count();
    if !has_terminal_punct && word_count >= 8 {
        out.push('.');
    }

    out
}

fn capitalize_sentence_starts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cap_next = true;
    for ch in text.chars() {
        if cap_next && ch.is_ascii_alphabetic() {
            out.push(ch.to_ascii_uppercase());
            cap_next = false;
        } else {
            out.push(ch);
            if ch.is_ascii_alphabetic() {
                cap_next = false;
            }
        }
        if matches!(ch, '.' | '!' | '?') {
            cap_next = true;
        }
    }
    out
}

fn is_degenerate_transcript_text(text: &str) -> bool {
    let words: Vec<String> = text
        .split_whitespace()
        .map(normalize_word_for_repetition)
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() < 6 {
        return false;
    }

    let unique: HashSet<&str> = words.iter().map(|w| w.as_str()).collect();
    if unique.len() <= 2 && words.len() >= 6 {
        return true;
    }
    if words.len() >= 12 && unique.len().saturating_mul(100) / words.len() <= 30 {
        return true;
    }

    if max_same_word_run(&words) >= 4 {
        return true;
    }

    has_repeating_phrase_words(&words, 1, 3)
        || has_repeating_phrase_words(&words, 2, 3)
        || has_repeating_phrase_words(&words, 3, 3)
}

fn is_low_quality_transcript_text(text: &str, audio_seconds: f32) -> bool {
    if is_degenerate_transcript_text(text) {
        return true;
    }
    if has_digit_hallucination(text) {
        return true;
    }
    let words = text.split_whitespace().count();
    if audio_seconds >= 8.0 && words <= 1 {
        return true;
    }
    if audio_seconds >= 14.0 && words <= 2 {
        return true;
    }
    false
}

fn has_digit_hallucination(text: &str) -> bool {
    let mut same_digit_run = 0usize;
    let mut last_digit: Option<char> = None;
    for c in text.chars() {
        if c.is_ascii_digit() {
            if Some(c) == last_digit {
                same_digit_run += 1;
            } else {
                same_digit_run = 1;
                last_digit = Some(c);
            }
            if same_digit_run >= 5 {
                return true;
            }
        } else {
            same_digit_run = 0;
            last_digit = None;
        }
    }

    for token in text.split_whitespace() {
        let digits_only: String = token.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits_only.len() >= 6 {
            let unique: std::collections::HashSet<char> = digits_only.chars().collect();
            if unique.len() == 1 {
                return true;
            }
        }
    }
    false
}

fn normalize_word_for_repetition(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '\'')
        .collect::<String>()
        .to_ascii_lowercase()
}

fn max_same_word_run(words: &[String]) -> usize {
    if words.is_empty() {
        return 0;
    }
    let mut max_run = 1usize;
    let mut run = 1usize;
    for i in 1..words.len() {
        if words[i] == words[i - 1] {
            run += 1;
            max_run = max_run.max(run);
        } else {
            run = 1;
        }
    }
    max_run
}

fn has_repeating_phrase_words(words: &[String], phrase_len: usize, repeats: usize) -> bool {
    if phrase_len == 0 || repeats < 2 {
        return false;
    }
    let span = phrase_len * repeats;
    if words.len() < span {
        return false;
    }

    for start in 0..=words.len() - span {
        let base = &words[start..start + phrase_len];
        let mut ok = true;
        for r in 1..repeats {
            let s = start + r * phrase_len;
            let e = s + phrase_len;
            if &words[s..e] != base {
                ok = false;
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}

fn likely_truncated_transcript(text: &str, audio_seconds: f32) -> bool {
    let words = text.split_whitespace().count();
    if audio_seconds >= 10.0 && words <= 8 {
        return true;
    }
    if audio_seconds >= 6.0 && words <= 4 {
        return true;
    }
    false
}

fn phrase_bias_token_ids(tokenizer: &Tokenizer, terms: &[String]) -> HashSet<i64> {
    if terms.is_empty() {
        return HashSet::new();
    }
    let mut out = HashSet::new();
    for term in terms {
        for form in [
            term.clone(),
            format!(" {term}"),
            term.to_ascii_uppercase(),
            format!(" {}", term.to_ascii_uppercase()),
        ] {
            if let Some(id) = tokenizer.token_to_id(&form) {
                out.insert(id as i64);
            }
        }
    }
    out
}

fn transcript_quality_score(text: &str, audio_seconds: f32) -> f32 {
    let words = text.split_whitespace().count() as f32;
    let chars = text.chars().count() as f32;
    let punctuation_bonus = if text.ends_with('.') || text.ends_with('!') || text.ends_with('?') {
        0.15
    } else {
        0.0
    };
    let truncation_penalty = if likely_truncated_transcript(text, audio_seconds) {
        0.8
    } else {
        0.0
    };
    let repetition_penalty = if is_degenerate_transcript_text(text) {
        2.2
    } else {
        0.0
    };
    (words * 0.55 + chars * 0.015 + punctuation_bonus) - truncation_penalty - repetition_penalty
}

fn estimate_segment_confidence(text: &str, audio_seconds: f32, partial: bool) -> Option<f32> {
    if partial || text.trim().is_empty() {
        return None;
    }
    let words = text.split_whitespace().count() as f32;
    let mut confidence = 0.52 + (words.min(18.0) * 0.02);
    if likely_truncated_transcript(text, audio_seconds) {
        confidence -= 0.18;
    }
    if is_low_quality_transcript_text(text, audio_seconds) {
        confidence -= 0.24;
    }
    Some(confidence.clamp(0.05, 0.98))
}

fn openai_cloud_fallback_text(samples: &[f32], sample_rate: u32) -> Option<String> {
    if !cloud_fallback_enabled() {
        return None;
    }
    let api_key = std::env::var("DICTUM_OPENAI_API_KEY").ok()?;
    if api_key.trim().is_empty() || samples.is_empty() {
        return None;
    }

    let prepared = prepare_cloud_samples(samples, sample_rate);
    if prepared.is_empty() {
        return None;
    }

    let tmp_name = format!(
        "dictum-openai-fallback-{}-{}.wav",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis()
    );
    let wav_path = std::env::temp_dir().join(tmp_name);
    if let Err(e) = write_pcm16_wav(&wav_path, &prepared, sample_rate) {
        warn!(error = %e, "cloud fallback wav write failed");
        return None;
    }

    let wav_bytes = match std::fs::read(&wav_path) {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_file(&wav_path);
            warn!(error = %e, "cloud fallback wav read failed");
            return None;
        }
    };
    let _ = std::fs::remove_file(&wav_path);

    let file_part = match multipart::Part::bytes(wav_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
    {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "cloud fallback multipart file part failed");
            return None;
        }
    };
    let form = multipart::Form::new()
        .text("model", "gpt-4o-mini-transcribe")
        .text("response_format", "json")
        .part("file", file_part);

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "cloud fallback client build failed");
            return None;
        }
    };

    let response = match client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .bearer_auth(api_key)
        .multipart(form)
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "cloud fallback request failed");
            return None;
        }
    };

    if !response.status().is_success() {
        warn!(
            status = %response.status(),
            "cloud fallback request returned non-success status"
        );
        return None;
    }

    let payload: serde_json::Value = match response.json() {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "cloud fallback json parse failed");
            return None;
        }
    };
    let text = payload.get("text")?.as_str()?.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn prepare_cloud_samples(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() {
        return vec![];
    }

    let gate = 0.0025f32;
    let first = samples.iter().position(|s| s.abs() >= gate).unwrap_or(0);
    let last = samples
        .iter()
        .rposition(|s| s.abs() >= gate)
        .unwrap_or(samples.len().saturating_sub(1));
    if first > last {
        return vec![];
    }
    let pad = (sample_rate as usize / 4).max(1); // ~250 ms context
    let start = first.saturating_sub(pad);
    let end = (last + pad).min(samples.len().saturating_sub(1));
    let mut out = samples[start..=end].to_vec();
    normalize_rms_in_place(&mut out, 0.12);
    out
}

#[cfg(target_os = "windows")]
fn windows_dictation_fallback_text(samples: &[f32], sample_rate: u32) -> Option<String> {
    if samples.is_empty() {
        return None;
    }
    let prepared = prepare_fallback_samples(samples, sample_rate);
    if prepared.is_empty() {
        return None;
    }
    debug!(
        raw_samples = samples.len(),
        prepared_samples = prepared.len(),
        sample_rate,
        "running windows dictation fallback"
    );

    let tmp_name = format!(
        "dictum-fallback-{}-{}.wav",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis()
    );
    let wav_path = std::env::temp_dir().join(tmp_name);

    if let Err(e) = write_pcm16_wav(&wav_path, &prepared, sample_rate) {
        debug!(error = %e, "failed to write fallback wav");
        return None;
    }

    let escaped = wav_path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         Add-Type -AssemblyName System.Speech; \
         $ri=[System.Speech.Recognition.SpeechRecognitionEngine]::InstalledRecognizers() | Select-Object -First 1; \
         if ($null -eq $ri) {{ exit 0 }}; \
         $r=New-Object System.Speech.Recognition.SpeechRecognitionEngine($ri.Culture); \
         $r.InitialSilenceTimeout=[TimeSpan]::FromSeconds(1); \
         $r.BabbleTimeout=[TimeSpan]::FromSeconds(5); \
         $r.EndSilenceTimeout=[TimeSpan]::FromMilliseconds(400); \
         $r.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar)); \
         $r.SetInputToWaveFile('{escaped}'); \
         $parts=New-Object System.Collections.Generic.List[string]; \
         while ($true) {{ \
           $res=$r.Recognize([TimeSpan]::FromSeconds(6)); \
           if ($null -eq $res) {{ break }}; \
           if (-not [string]::IsNullOrWhiteSpace($res.Text)) {{ [void]$parts.Add($res.Text.Trim()) }}; \
         }}; \
         if ($parts.Count -gt 0) {{ [Console]::OutputEncoding=[System.Text.Encoding]::UTF8; Write-Output ($parts -join ' ') }}",
    );

    let output = run_powershell_script(&script);

    let _ = std::fs::remove_file(&wav_path);

    let Ok(output) = output else {
        warn!("windows dictation fallback: failed to start PowerShell");
        return None;
    };
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !text.is_empty() {
        if is_redacted_asterisk_text(&text) {
            warn!("windows dictation fallback produced redacted output; dropping it");
            return None;
        }
        if !output.status.success() {
            warn!(
                code = output.status.code(),
                "windows dictation fallback produced text with non-zero exit status"
            );
        }
        return Some(text);
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(
            code = output.status.code(),
            stderr = %stderr.trim(),
            "windows dictation fallback failed with empty output"
        );
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn windows_dictation_fallback_text(_samples: &[f32], _sample_rate: u32) -> Option<String> {
    None
}

fn is_redacted_asterisk_text(text: &str) -> bool {
    let mut total = 0usize;
    let mut stars = 0usize;
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        if c == '*' {
            stars += 1;
        }
    }
    total >= 6 && stars.saturating_mul(100) / total >= 80
}

#[cfg(target_os = "windows")]
fn prepare_fallback_samples(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() {
        return vec![];
    }

    let gate = 0.0035f32;
    let first = samples.iter().position(|s| s.abs() >= gate).unwrap_or(0);
    let last = samples
        .iter()
        .rposition(|s| s.abs() >= gate)
        .unwrap_or(samples.len().saturating_sub(1));

    if first > last {
        return vec![];
    }

    let pad = (sample_rate as usize / 5).max(1); // ~200ms context
    let start = first.saturating_sub(pad);
    let end = (last + pad).min(samples.len().saturating_sub(1));
    let mut out = samples[start..=end].to_vec();

    // System.Speech tends to be brittle on low-volume input.
    normalize_rms_in_place(&mut out, 0.12);
    out
}

#[cfg(target_os = "windows")]
fn run_powershell_script(script: &str) -> std::io::Result<std::process::Output> {
    let candidates = [
        std::path::PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
        std::path::PathBuf::from("powershell.exe"),
        std::path::PathBuf::from("pwsh.exe"),
    ];

    let args = [
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ];

    let mut last_err: Option<std::io::Error> = None;
    for exe in candidates {
        match std::process::Command::new(&exe).args(args).output() {
            Ok(out) => return Ok(out),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("no PowerShell runtime found")))
}

fn write_pcm16_wav(path: &Path, samples: &[f32], sample_rate: u32) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;

    let data_len = (samples.len() * 2) as u32;
    let riff_len = 36u32 + data_len;

    use std::io::Write;
    file.write_all(b"RIFF")?;
    file.write_all(&riff_len.to_le_bytes())?;
    file.write_all(b"WAVE")?;

    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?; // PCM fmt chunk size
    file.write_all(&1u16.to_le_bytes())?; // PCM
    file.write_all(&1u16.to_le_bytes())?; // mono
    file.write_all(&sample_rate.to_le_bytes())?;
    let byte_rate = sample_rate * 2;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?; // block align
    file.write_all(&16u16.to_le_bytes())?; // bits per sample

    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    for &sample in samples {
        let v = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        file.write_all(&v.to_le_bytes())?;
    }
    Ok(())
}

fn build_hann_window(n: usize) -> Vec<f32> {
    use std::f32::consts::PI;
    (0..n)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos()))
        .collect()
}

fn build_mel_filters(
    fft_size: usize,
    sr: u32,
    n_mels: usize,
    fmin: f32,
    fmax: f32,
) -> Vec<Vec<f32>> {
    let n_freqs = fft_size / 2 + 1;
    let mel_min = hz_to_mel_slaney(fmin);
    let mel_max = hz_to_mel_slaney(fmax);

    let mel_pts: Vec<f32> = (0..=(n_mels + 1))
        .map(|i| mel_min + (mel_max - mel_min) * i as f32 / (n_mels + 1) as f32)
        .collect();

    let hz_pts: Vec<f32> = mel_pts.iter().map(|&m| mel_to_hz_slaney(m)).collect();
    let fft_freqs: Vec<f32> = (0..n_freqs)
        .map(|k| k as f32 * sr as f32 / fft_size as f32)
        .collect();

    let mut filters = vec![vec![0f32; n_freqs]; n_mels];
    for m in 0..n_mels {
        let lower = hz_pts[m];
        let center = hz_pts[m + 1];
        let upper = hz_pts[m + 2];
        let down_denom = (center - lower).max(1e-10);
        let up_denom = (upper - center).max(1e-10);
        let enorm = 2.0 / (upper - lower).max(1e-10);

        for (k, &freq) in fft_freqs.iter().enumerate() {
            let w = if freq >= lower && freq <= center {
                (freq - lower) / down_denom
            } else if freq > center && freq <= upper {
                (upper - freq) / up_denom
            } else {
                0.0
            };
            filters[m][k] = (w * enorm).max(0.0);
        }
    }
    filters
}

fn normalize_rms_in_place(samples: &mut [f32], target_rms: f32) {
    if samples.is_empty() {
        return;
    }
    let sum_sq = samples.iter().map(|s| s * s).sum::<f32>();
    let rms = (sum_sq / samples.len() as f32).sqrt();
    if rms <= 1e-6 {
        return;
    }
    let gain = (target_rms / rms).clamp(0.8, 15.0);
    if (gain - 1.0).abs() < 1e-3 {
        return;
    }
    for s in samples.iter_mut() {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }
}

fn reflect_pad(samples: &[f32], pad: usize) -> Vec<f32> {
    if pad == 0 {
        return samples.to_vec();
    }
    if samples.is_empty() {
        return vec![0.0; pad * 2];
    }
    if samples.len() == 1 {
        return vec![samples[0]; samples.len() + pad * 2];
    }

    let n = samples.len() as isize;
    let mut out = Vec::with_capacity(samples.len() + 2 * pad);
    for i in -(pad as isize)..(n + pad as isize) {
        let idx = reflect_index(i, samples.len());
        out.push(samples[idx]);
    }
    out
}

fn reflect_index(mut i: isize, len: usize) -> usize {
    let max = len as isize - 1;
    while i < 0 || i > max {
        if i < 0 {
            i = -i;
        } else {
            i = 2 * max - i;
        }
    }
    i as usize
}

fn hz_to_mel_slaney(hz: f32) -> f32 {
    let f_sp = 200.0 / 3.0;
    let min_log_hz = 1_000.0;
    let min_log_mel = min_log_hz / f_sp; // 15
    let logstep = (6.4_f32).ln() / 27.0;
    if hz >= min_log_hz {
        min_log_mel + (hz / min_log_hz).ln() / logstep
    } else {
        hz / f_sp
    }
}

fn mel_to_hz_slaney(mel: f32) -> f32 {
    let f_sp = 200.0 / 3.0;
    let min_log_hz = 1_000.0;
    let min_log_mel = min_log_hz / f_sp; // 15
    let logstep = (6.4_f32).ln() / 27.0;
    if mel >= min_log_mel {
        min_log_hz * (logstep * (mel - min_log_mel)).exp()
    } else {
        mel * f_sp
    }
}
