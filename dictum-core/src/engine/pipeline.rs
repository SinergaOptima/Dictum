//! Blocking pipeline loop.
//!
//! ## Pipeline stages (per iteration)
//!
//! ```text
//! 1. Drain ring buffer → Vec<f32> (one chunk per iteration)
//! 2. Build AudioChunk at the capture sample rate
//! 3. VAD classify → Speech | Silence
//! 4. Accumulate speech samples; flush on Silence or max_speech_samples
//! 5. When accumulation is flushed:
//!    a. Optionally emit partial transcript updates (if enabled)
//!    b. Emit a final transcript
//!    c. Assign stable utterance IDs across partial → final updates
//! 6. Broadcast TranscriptEvent on the channel
//! ```
//!
//! This entire loop runs in `spawn_blocking`, keeping the Tokio async
//! executor free for I/O (Tauri IPC, file system, etc.).

use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::broadcast;
use tracing::{debug, error, info, info_span, warn, Span};

use crate::{
    audio::resample::RateConverter,
    buffering::{chunk::AudioChunk, AudioConsumer, Consumer},
    engine::EngineConfig,
    inference::ModelHandle,
    ipc::events::{
        AudioActivityEvent, EngineStatus, EngineStatusEvent, SegmentKind, TranscriptEvent,
        TranscriptSegment,
    },
    vad::{VadDecision, VoiceActivityDetector},
};

pub struct PipelineDiagnostics {
    pub frames_in: AtomicUsize,
    pub frames_resampled: AtomicUsize,
    pub vad_windows: AtomicUsize,
    pub vad_speech: AtomicUsize,
    pub inference_calls: AtomicUsize,
    pub inference_errors: AtomicUsize,
    pub segments_emitted: AtomicUsize,
    pub fallback_emitted: AtomicUsize,
}

impl Default for PipelineDiagnostics {
    fn default() -> Self {
        Self {
            frames_in: AtomicUsize::new(0),
            frames_resampled: AtomicUsize::new(0),
            vad_windows: AtomicUsize::new(0),
            vad_speech: AtomicUsize::new(0),
            inference_calls: AtomicUsize::new(0),
            inference_errors: AtomicUsize::new(0),
            segments_emitted: AtomicUsize::new(0),
            fallback_emitted: AtomicUsize::new(0),
        }
    }
}

impl PipelineDiagnostics {
    pub fn reset(&self) {
        self.frames_in.store(0, Ordering::Relaxed);
        self.frames_resampled.store(0, Ordering::Relaxed);
        self.vad_windows.store(0, Ordering::Relaxed);
        self.vad_speech.store(0, Ordering::Relaxed);
        self.inference_calls.store(0, Ordering::Relaxed);
        self.inference_errors.store(0, Ordering::Relaxed);
        self.segments_emitted.store(0, Ordering::Relaxed);
        self.fallback_emitted.store(0, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot {
            frames_in: self.frames_in.load(Ordering::Relaxed),
            frames_resampled: self.frames_resampled.load(Ordering::Relaxed),
            vad_windows: self.vad_windows.load(Ordering::Relaxed),
            vad_speech: self.vad_speech.load(Ordering::Relaxed),
            inference_calls: self.inference_calls.load(Ordering::Relaxed),
            inference_errors: self.inference_errors.load(Ordering::Relaxed),
            segments_emitted: self.segments_emitted.load(Ordering::Relaxed),
            fallback_emitted: self.fallback_emitted.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DiagnosticsSnapshot {
    pub frames_in: usize,
    pub frames_resampled: usize,
    pub vad_windows: usize,
    pub vad_speech: usize,
    pub inference_calls: usize,
    pub inference_errors: usize,
    pub segments_emitted: usize,
    pub fallback_emitted: usize,
}

/// All context the pipeline needs, passed as one struct so the closure stays tidy.
pub struct PipelineContext {
    pub config: EngineConfig,
    pub model: ModelHandle,
    pub vad: Box<dyn VoiceActivityDetector>,
    pub consumer: AudioConsumer,
    pub running: Arc<AtomicBool>,
    pub transcript_tx: broadcast::Sender<TranscriptEvent>,
    pub status_tx: broadcast::Sender<EngineStatusEvent>,
    pub activity_tx: broadcast::Sender<AudioActivityEvent>,
    pub status: Arc<Mutex<EngineStatus>>,
    pub seq: Arc<AtomicU64>,
    pub capture_sample_rate: u32,
    pub diagnostics: Arc<PipelineDiagnostics>,
}

/// Chunk size drained from the ring buffer per iteration.
/// 20 ms at 48 kHz = 960 samples; at 16 kHz = 320 samples.
/// Using 960 gives a reasonable VAD frame stride for most capture rates.
const DRAIN_CHUNK: usize = 960;

/// Minimum sleep when the ring is empty (avoids busy-wait burning a core).
const DEFAULT_SLEEP_EMPTY_MS: u64 = 5;
const EMPTY_FINAL_STREAK_FOR_FALLBACK: usize = 2;
const FALLBACK_TEXT: &str = "[speech captured]";
const STOP_FALLBACK_RMS_ACTIVITY_FACTOR: usize = 2; // min_speech_samples / 2
const PARTIAL_MIN_INTERVAL_MS: u64 = 900;
const PARTIAL_MIN_NEW_SAMPLES: usize = 12_000;
const MAX_FLUSH_RETRY_TAIL_SECONDS: usize = 12;
const MAX_FLUSH_CONTINUATION_OVERLAP_MS: usize = 1_600;

/// Run the blocking pipeline until `ctx.running` becomes false.
pub fn run(mut ctx: PipelineContext) {
    info!("pipeline started");

    // Initialise resampler (passthrough when rates match)
    let mut resampler = match RateConverter::new(
        ctx.capture_sample_rate,
        ctx.config.target_sample_rate,
        DRAIN_CHUNK,
    ) {
        Ok(r) => r,
        Err(e) => {
            error!("failed to create resampler: {e}");
            return;
        }
    };

    if !resampler.is_passthrough() {
        info!(
            "resampling enabled from={} to={}",
            ctx.capture_sample_rate, ctx.config.target_sample_rate
        );
    }

    // Temporary scratch buffer (stack allocation, reused each iteration)
    let mut raw = vec![0f32; DRAIN_CHUNK];
    // Accumulated speech samples awaiting inference
    let mut speech_buf: Vec<f32> = Vec::with_capacity(ctx.config.max_speech_samples);
    // Rolling audio window (up to max_speech_samples), used as a stop-time
    // rescue inference source when VAD fails to mark speech.
    let mut recent_audio_buf: Vec<f32> = Vec::with_capacity(ctx.config.max_speech_samples);
    // Whether we were in speech on the previous frame
    let mut was_speech = false;
    // Stable utterance ID for the current speech region.
    let mut active_utterance_id: Option<String> = None;
    let mut next_utterance_id = 0u64;
    // Independent sequence for activity events.
    let mut activity_seq = 0u64;
    // Utterance span for tracing
    let mut utterance_span: Option<Span> = None;
    // Consecutive final inference calls that produced empty output.
    let mut empty_final_streak = 0usize;
    // Count of final outputs (real or fallback) emitted in this session.
    let mut final_output_count = 0usize;
    // Samples with elevated RMS, independent of VAD decisions.
    let mut rms_active_samples = 0usize;
    // Partial inference throttling for long speech regions.
    let mut last_partial_infer_at: Option<Instant> = None;
    let mut last_partial_infer_samples = 0usize;
    // Speech accumulated since the last successful final emission.
    let mut new_speech_samples_since_final = 0usize;

    loop {
        // ── 0. Check running flag ─────────────────────────────────────────
        if !ctx.running.load(Ordering::Relaxed) {
            break;
        }

        // ── 1. Drain ring buffer ──────────────────────────────────────────
        let n = ctx.consumer.pop_slice(&mut raw);

        if n == 0 {
            // Nothing to process — yield to avoid burning 100 % CPU
            std::thread::sleep(std::time::Duration::from_millis(empty_sleep_ms()));
            continue;
        }

        ctx.diagnostics.frames_in.fetch_add(n, Ordering::Relaxed);

        // ── 2. Resample to target rate ────────────────────────────────────
        let resampled = resampler.process(&raw[..n]);
        if resampled.is_empty() {
            // Partial chunk — waiting for more data to fill rubato's input buffer
            continue;
        }
        ctx.diagnostics
            .frames_resampled
            .fetch_add(resampled.len(), Ordering::Relaxed);
        let mut chunk = AudioChunk::new(resampled, ctx.config.target_sample_rate);
        apply_adaptive_input_gain(&mut chunk.samples, ctx.config.vad_threshold);
        append_rolling_samples(
            &mut recent_audio_buf,
            &chunk.samples,
            ctx.config.max_speech_samples,
        );

        debug!(
            raw = n,
            resampled = chunk.samples.len(),
            "processed audio chunk"
        );

        // ── 3. VAD ───────────────────────────────────────────────────────
        ctx.diagnostics.vad_windows.fetch_add(1, Ordering::Relaxed);
        let rms = compute_rms(&chunk.samples);
        if rms >= ctx.config.vad_threshold {
            rms_active_samples = rms_active_samples.saturating_add(chunk.samples.len());
        }
        let decision = ctx.vad.classify(&chunk);
        let is_speech = matches!(decision, VadDecision::Speech);
        if is_speech {
            ctx.diagnostics.vad_speech.fetch_add(1, Ordering::Relaxed);
        }
        let activity = AudioActivityEvent {
            seq: activity_seq,
            rms,
            is_speech,
        };
        activity_seq = activity_seq.saturating_add(1);
        let _ = ctx.activity_tx.send(activity);

        // Log audio level periodically for diagnostics
        if activity_seq % 50 == 0 {
            debug!(
                rms = format_args!("{:.4}", rms),
                is_speech,
                speech_buf_len = speech_buf.len(),
                min_samples = ctx.config.min_speech_samples,
                "audio level check"
            );
        }

        match decision {
            VadDecision::Speech => {
                was_speech = true;
                speech_buf.extend_from_slice(&chunk.samples);
                new_speech_samples_since_final =
                    new_speech_samples_since_final.saturating_add(chunk.samples.len());

                if active_utterance_id.is_none() {
                    let uid = format!("utt-{}", next_utterance_id);
                    next_utterance_id += 1;
                    active_utterance_id = Some(uid.clone());
                    last_partial_infer_at = None;
                    last_partial_infer_samples = 0;
                    let span = info_span!(
                        "utterance",
                        utterance_id = %uid,
                        capture_rate = ctx.capture_sample_rate,
                        target_rate = ctx.config.target_sample_rate,
                    );
                    utterance_span = Some(span);
                }

                if let Some(ref span) = utterance_span {
                    let _enter = span.enter();
                    debug!(samples = speech_buf.len(), "speech accumulating");
                }

                if speech_buf.len() >= ctx.config.max_speech_samples {
                    warn!("max_speech_samples reached — forcing inference flush");
                    let outcome = flush_inference(
                        &mut ctx,
                        &speech_buf,
                        false,
                        active_utterance_id.as_deref(),
                    );
                    let emitted_primary = matches!(&outcome, FlushOutcome::Emitted);
                    if handle_final_flush_result(
                        &mut ctx,
                        outcome,
                        active_utterance_id.as_deref(),
                        &mut empty_final_streak,
                    ) {
                        final_output_count = final_output_count.saturating_add(1);
                        new_speech_samples_since_final = 0;
                    }
                    if emitted_primary {
                        let continuation_overlap_samples = (ctx.config.target_sample_rate as usize)
                            .saturating_mul(MAX_FLUSH_CONTINUATION_OVERLAP_MS)
                            / 1000;
                        retain_tail_samples(&mut speech_buf, continuation_overlap_samples.max(1));
                        active_utterance_id = None;
                        utterance_span = None;
                        last_partial_infer_at = Some(Instant::now());
                        last_partial_infer_samples = 0;
                        was_speech = true;
                    } else {
                        let retry_tail_samples = (ctx.config.target_sample_rate as usize)
                            .saturating_mul(MAX_FLUSH_RETRY_TAIL_SECONDS)
                            .max(ctx.config.min_speech_samples);
                        retain_tail_samples(&mut speech_buf, retry_tail_samples);
                        last_partial_infer_at = Some(Instant::now());
                        last_partial_infer_samples = speech_buf.len();
                        warn!(
                            retained_samples = speech_buf.len(),
                            retry_tail_samples,
                            "max-length flush yielded fallback/empty; retaining tail for retry to avoid losing long utterance context"
                        );
                    }
                } else if ctx.config.enable_partial_inference
                    && speech_buf.len() >= ctx.config.min_speech_samples
                {
                    let now = Instant::now();
                    let enough_time = last_partial_infer_at
                        .map(|t| {
                            now.duration_since(t) >= Duration::from_millis(PARTIAL_MIN_INTERVAL_MS)
                        })
                        .unwrap_or(true);
                    let new_samples = speech_buf.len().saturating_sub(last_partial_infer_samples);
                    let partial_delta_threshold =
                        PARTIAL_MIN_NEW_SAMPLES.min(ctx.config.min_speech_samples.max(1));
                    if enough_time && new_samples >= partial_delta_threshold {
                        flush_inference(
                            &mut ctx,
                            &speech_buf,
                            true,
                            active_utterance_id.as_deref(),
                        );
                        last_partial_infer_at = Some(now);
                        last_partial_infer_samples = speech_buf.len();
                    }
                }
            }

            VadDecision::Silence => {
                if was_speech && speech_buf.len() >= ctx.config.min_speech_samples {
                    debug!(
                        samples = speech_buf.len(),
                        "end of utterance — running final inference"
                    );
                    let outcome = flush_inference(
                        &mut ctx,
                        &speech_buf,
                        false,
                        active_utterance_id.as_deref(),
                    );
                    if handle_final_flush_result(
                        &mut ctx,
                        outcome,
                        active_utterance_id.as_deref(),
                        &mut empty_final_streak,
                    ) {
                        final_output_count = final_output_count.saturating_add(1);
                        new_speech_samples_since_final = 0;
                    }
                }
                if was_speech {
                    speech_buf.clear();
                    ctx.vad.reset();
                    ctx.model.0.lock().reset();
                    active_utterance_id = None;
                    utterance_span = None;
                    last_partial_infer_at = None;
                    last_partial_infer_samples = 0;
                    new_speech_samples_since_final = 0;
                }
                was_speech = false;
            }
        }
    }

    // Force a terminal final flush on stop to avoid losing speech when the
    // user releases push-to-talk / toggles stop before silence is detected.
    if !speech_buf.is_empty() {
        if new_speech_samples_since_final > 0 || final_output_count == 0 {
            info!(
                utterance_id = ?active_utterance_id,
                buffered_samples = speech_buf.len(),
                "stop requested with buffered speech — forcing final flush"
            );
            let outcome =
                flush_inference(&mut ctx, &speech_buf, false, active_utterance_id.as_deref());
            if handle_final_flush_result(
                &mut ctx,
                outcome,
                active_utterance_id.as_deref(),
                &mut empty_final_streak,
            ) {
                final_output_count = final_output_count.saturating_add(1);
            }
        } else {
            debug!(
                buffered_samples = speech_buf.len(),
                "stop requested with overlap-only buffer; skipping duplicate final flush"
            );
        }
        speech_buf.clear();
        ctx.vad.reset();
        ctx.model.0.lock().reset();
    }

    // Safety net: if we had meaningful RMS activity but emitted no final output,
    // run one rescue final inference from the recent rolling audio buffer.
    let rms_fallback_threshold = ctx.config.min_speech_samples / STOP_FALLBACK_RMS_ACTIVITY_FACTOR;
    if final_output_count == 0 && rms_active_samples >= rms_fallback_threshold {
        warn!(
            rms_active_samples,
            rms_fallback_threshold,
            rescue_samples = recent_audio_buf.len(),
            "no final output emitted despite sustained RMS activity — attempting rescue final inference"
        );
        if !recent_audio_buf.is_empty() {
            let outcome = flush_inference(&mut ctx, &recent_audio_buf, false, None);
            if handle_final_flush_result(&mut ctx, outcome, None, &mut empty_final_streak) {
                final_output_count = final_output_count.saturating_add(1);
            }
        }
    }

    // Last resort placeholder only when rescue inference also failed.
    if final_output_count == 0 && rms_active_samples >= rms_fallback_threshold {
        warn!(
            rms_active_samples,
            rms_fallback_threshold,
            "rescue inference produced no final output — forcing fallback segment"
        );
        emit_fallback_event(&mut ctx, None);
    }

    let snap = ctx.diagnostics.snapshot();
    info!(
        frames_in = snap.frames_in,
        frames_resampled = snap.frames_resampled,
        vad_windows = snap.vad_windows,
        vad_speech = snap.vad_speech,
        inference_calls = snap.inference_calls,
        inference_errors = snap.inference_errors,
        segments_emitted = snap.segments_emitted,
        fallback_emitted = snap.fallback_emitted,
        "pipeline stopped — diagnostics"
    );
}

fn empty_sleep_ms() -> u64 {
    static EMPTY_SLEEP_MS: OnceLock<u64> = OnceLock::new();
    *EMPTY_SLEEP_MS.get_or_init(|| {
        std::env::var("DICTUM_PIPELINE_EMPTY_SLEEP_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(|v| v.clamp(1, 20))
            .unwrap_or(DEFAULT_SLEEP_EMPTY_MS)
    })
}

/// Run inference on `samples` and broadcast the result.
enum FlushOutcome {
    Emitted,
    Empty,
    Error,
}

fn flush_inference(
    ctx: &mut PipelineContext,
    samples: &[f32],
    partial: bool,
    utterance_id: Option<&str>,
) -> FlushOutcome {
    ctx.diagnostics
        .inference_calls
        .fetch_add(1, Ordering::Relaxed);

    let samples_len = samples.len();
    let chunk = AudioChunk::new(samples.to_vec(), ctx.config.target_sample_rate);

    let mut segments = {
        let mut model = ctx.model.0.lock();
        match model.transcribe(&chunk, partial) {
            Ok(segs) => segs,
            Err(e) => {
                ctx.diagnostics
                    .inference_errors
                    .fetch_add(1, Ordering::Relaxed);
                error!(utterance_id = ?utterance_id, error = %e, "inference error");
                return FlushOutcome::Error;
            }
        }
    };

    if segments.is_empty() {
        info!(
            utterance_id = ?utterance_id,
            samples = samples_len,
            partial,
            "inference returned empty segments — model may not be loaded or audio too short"
        );
        return FlushOutcome::Empty;
    }

    ctx.diagnostics
        .segments_emitted
        .fetch_add(segments.len(), Ordering::Relaxed);

    if let Some(utterance_id) = utterance_id {
        for segment in &mut segments {
            segment.id = utterance_id.to_string();
        }
    }

    let text_preview: String = segments
        .iter()
        .map(|s| s.text.chars().take(50).collect::<String>())
        .collect::<Vec<_>>()
        .join(" | ");

    let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
    let event = TranscriptEvent { seq, segments };

    let emit_result = ctx.transcript_tx.send(event);
    info!(
        utterance_id = ?utterance_id,
        samples = samples_len,
        partial,
        text_preview = %text_preview,
        emit_success = emit_result.is_ok(),
        "transcript emitted"
    );
    FlushOutcome::Emitted
}

fn handle_final_flush_result(
    ctx: &mut PipelineContext,
    outcome: FlushOutcome,
    utterance_id: Option<&str>,
    empty_final_streak: &mut usize,
) -> bool {
    match outcome {
        FlushOutcome::Emitted => {
            if *empty_final_streak > 0 {
                *empty_final_streak = 0;
                let _ = ctx.status_tx.send(EngineStatusEvent {
                    status: EngineStatus::Listening,
                    detail: None,
                });
            }
            true
        }
        FlushOutcome::Empty => {
            *empty_final_streak = empty_final_streak.saturating_add(1);
            warn!(
                utterance_id = ?utterance_id,
                empty_final_streak = *empty_final_streak,
                "final inference produced empty output"
            );

            if *empty_final_streak >= EMPTY_FINAL_STREAK_FOR_FALLBACK {
                emit_fallback_event(ctx, utterance_id);
                let _ = ctx.status_tx.send(EngineStatusEvent {
                    status: EngineStatus::Listening,
                    detail: Some(
                        "Transcription degraded: speech detected but model returned empty output; using fallback."
                            .into(),
                    ),
                });
                true
            } else {
                false
            }
        }
        FlushOutcome::Error => {
            emit_fallback_event(ctx, utterance_id);
            let _ = ctx.status_tx.send(EngineStatusEvent {
                status: EngineStatus::Listening,
                detail: Some("Transcription error: inference failed during finalization.".into()),
            });
            true
        }
    }
}

fn emit_fallback_event(ctx: &mut PipelineContext, utterance_id: Option<&str>) {
    let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
    let fallback_id = utterance_id
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("fallback-{seq}"));
    let event = TranscriptEvent {
        seq,
        segments: vec![TranscriptSegment {
            id: fallback_id,
            text: FALLBACK_TEXT.to_string(),
            kind: SegmentKind::Final,
            confidence: None,
        }],
    };
    let emitted = ctx.transcript_tx.send(event).is_ok();
    if emitted {
        ctx.diagnostics
            .segments_emitted
            .fetch_add(1, Ordering::Relaxed);
        ctx.diagnostics
            .fallback_emitted
            .fetch_add(1, Ordering::Relaxed);
    }
    warn!(
        utterance_id = ?utterance_id,
        emitted,
        fallback_text = FALLBACK_TEXT,
        "emitted fallback transcript segment"
    );
}

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq = samples.iter().map(|s| s * s).sum::<f32>();
    (sum_sq / samples.len() as f32).sqrt()
}

fn apply_adaptive_input_gain(samples: &mut [f32], vad_threshold: f32) {
    if samples.is_empty() {
        return;
    }
    let rms = compute_rms(samples);
    if rms <= 3e-5 {
        return;
    }
    // Boost very quiet microphones/speakers toward a working speech band so
    // whisper-level input can still pass VAD and inference.
    let configured_boost = std::env::var("DICTUM_INPUT_GAIN_BOOST")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .map(|v| v.clamp(0.5, 8.0))
        .unwrap_or(1.0);
    let target_rms = (vad_threshold * 3.4 * configured_boost).clamp(0.012, 0.08);
    if rms >= target_rms {
        return;
    }
    let gain = (target_rms / rms).clamp(1.0, 9.0);
    if gain <= 1.03 {
        return;
    }
    for sample in samples.iter_mut() {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

fn append_rolling_samples(buf: &mut Vec<f32>, samples: &[f32], max_len: usize) {
    if max_len == 0 || samples.is_empty() {
        return;
    }
    if samples.len() >= max_len {
        buf.clear();
        buf.extend_from_slice(&samples[samples.len() - max_len..]);
        return;
    }

    let needed = buf.len().saturating_add(samples.len());
    if needed > max_len {
        let drop = needed - max_len;
        buf.drain(..drop);
    }
    buf.extend_from_slice(samples);
}

fn retain_tail_samples(buf: &mut Vec<f32>, tail_len: usize) {
    if tail_len == 0 {
        buf.clear();
        return;
    }
    if buf.len() <= tail_len {
        return;
    }
    let keep_from = buf.len() - tail_len;
    buf.drain(..keep_from);
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::AtomicUsize;
    use std::thread;
    use std::time::{Duration, Instant};

    use tokio::sync::broadcast::error::TryRecvError;

    use crate::buffering::{create_audio_ring, Producer};
    use crate::error::{DictumError, Result};
    use crate::inference::SpeechModel;
    use crate::ipc::events::{SegmentKind, TranscriptSegment};

    struct ScriptedVad {
        decisions: Vec<VadDecision>,
        idx: usize,
        resets: Arc<AtomicUsize>,
    }

    impl ScriptedVad {
        fn new(decisions: Vec<VadDecision>, resets: Arc<AtomicUsize>) -> Self {
            Self {
                decisions,
                idx: 0,
                resets,
            }
        }
    }

    impl VoiceActivityDetector for ScriptedVad {
        fn classify(&mut self, _chunk: &AudioChunk) -> VadDecision {
            let decision = self
                .decisions
                .get(self.idx)
                .copied()
                .unwrap_or(VadDecision::Silence);
            self.idx += 1;
            decision
        }

        fn reset(&mut self) {
            self.resets.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct TestModel {
        calls: Arc<Mutex<Vec<bool>>>,
        resets: Arc<AtomicUsize>,
        empty_partial: bool,
        empty_final: bool,
        fail_final: bool,
    }

    impl SpeechModel for TestModel {
        fn warm_up(&mut self) -> Result<()> {
            Ok(())
        }

        fn transcribe(
            &mut self,
            _chunk: &AudioChunk,
            partial: bool,
        ) -> Result<Vec<TranscriptSegment>> {
            self.calls.lock().push(partial);

            if partial && self.empty_partial {
                return Ok(vec![]);
            }
            if !partial && self.empty_final {
                return Ok(vec![]);
            }
            if !partial && self.fail_final {
                return Err(DictumError::Inference("intentional test failure".into()));
            }

            let kind = if partial {
                SegmentKind::Partial
            } else {
                SegmentKind::Final
            };

            Ok(vec![TranscriptSegment {
                id: "test-utterance".into(),
                text: if partial {
                    "partial".into()
                } else {
                    "final".into()
                },
                kind,
                confidence: None,
            }])
        }

        fn reset(&mut self) {
            self.resets.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn recv_event_with_timeout(
        rx: &mut broadcast::Receiver<TranscriptEvent>,
        timeout: Duration,
    ) -> TranscriptEvent {
        let start = Instant::now();
        loop {
            match rx.try_recv() {
                Ok(ev) => return ev,
                Err(TryRecvError::Empty) => {
                    if start.elapsed() >= timeout {
                        panic!("timed out waiting for transcript event");
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(TryRecvError::Lagged(_)) => continue,
                Err(TryRecvError::Closed) => panic!("transcript channel closed unexpectedly"),
            }
        }
    }

    fn assert_no_event_for(rx: &mut broadcast::Receiver<TranscriptEvent>, timeout: Duration) {
        let start = Instant::now();
        loop {
            match rx.try_recv() {
                Ok(ev) => panic!("expected no event, got seq={}", ev.seq),
                Err(TryRecvError::Empty) => {
                    if start.elapsed() >= timeout {
                        return;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(TryRecvError::Lagged(_)) => continue,
                Err(TryRecvError::Closed) => return,
            }
        }
    }

    fn base_config() -> EngineConfig {
        let mut cfg = EngineConfig::default();
        cfg.target_sample_rate = 16_000;
        cfg.min_speech_samples = 960;
        cfg.max_speech_samples = 8_000;
        cfg
    }

    #[test]
    fn flush_inference_emits_events_and_increments_seq() {
        let (_producer, consumer) = create_audio_ring();
        let (transcript_tx, mut transcript_rx) = broadcast::channel(8);
        let (status_tx, _) = broadcast::channel(8);
        let (activity_tx, _) = broadcast::channel(8);

        let calls = Arc::new(Mutex::new(Vec::new()));
        let model_resets = Arc::new(AtomicUsize::new(0));
        let model = ModelHandle::new(TestModel {
            calls,
            resets: model_resets,
            empty_partial: false,
            empty_final: false,
            fail_final: false,
        });

        let vad_resets = Arc::new(AtomicUsize::new(0));
        let vad: Box<dyn VoiceActivityDetector> = Box::new(ScriptedVad::new(vec![], vad_resets));

        let running = Arc::new(AtomicBool::new(true));
        let seq = Arc::new(AtomicU64::new(0));

        let mut ctx = PipelineContext {
            config: base_config(),
            model,
            vad,
            consumer,
            running,
            transcript_tx,
            status_tx,
            activity_tx,
            status: Arc::new(Mutex::new(EngineStatus::Idle)),
            seq: Arc::clone(&seq),
            capture_sample_rate: 16_000,
            diagnostics: Arc::new(PipelineDiagnostics::default()),
        };

        flush_inference(&mut ctx, &vec![0.1; 960], true, Some("utt-test"));
        flush_inference(&mut ctx, &vec![0.1; 960], false, Some("utt-test"));

        let first = recv_event_with_timeout(&mut transcript_rx, Duration::from_millis(200));
        let second = recv_event_with_timeout(&mut transcript_rx, Duration::from_millis(200));

        assert_eq!(first.seq, 0);
        assert_eq!(first.segments[0].kind, SegmentKind::Partial);
        assert_eq!(first.segments[0].id, "utt-test");
        assert_eq!(second.seq, 1);
        assert_eq!(second.segments[0].kind, SegmentKind::Final);
        assert_eq!(second.segments[0].id, "utt-test");
        assert_eq!(seq.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn flush_inference_skips_empty_and_error_results() {
        let (_producer, consumer) = create_audio_ring();
        let (transcript_tx, mut transcript_rx) = broadcast::channel(8);
        let (status_tx, _) = broadcast::channel(8);
        let (activity_tx, _) = broadcast::channel(8);

        let calls = Arc::new(Mutex::new(Vec::new()));
        let model_resets = Arc::new(AtomicUsize::new(0));
        let model = ModelHandle::new(TestModel {
            calls,
            resets: model_resets,
            empty_partial: true,
            empty_final: false,
            fail_final: true,
        });

        let vad_resets = Arc::new(AtomicUsize::new(0));
        let vad: Box<dyn VoiceActivityDetector> = Box::new(ScriptedVad::new(vec![], vad_resets));

        let seq = Arc::new(AtomicU64::new(0));
        let mut ctx = PipelineContext {
            config: base_config(),
            model,
            vad,
            consumer,
            running: Arc::new(AtomicBool::new(true)),
            transcript_tx,
            status_tx,
            activity_tx,
            status: Arc::new(Mutex::new(EngineStatus::Idle)),
            seq: Arc::clone(&seq),
            capture_sample_rate: 16_000,
            diagnostics: Arc::new(PipelineDiagnostics::default()),
        };

        flush_inference(&mut ctx, &vec![0.1; 960], true, Some("utt-test"));
        flush_inference(&mut ctx, &vec![0.1; 960], false, Some("utt-test"));

        assert_no_event_for(&mut transcript_rx, Duration::from_millis(100));
        assert_eq!(seq.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn run_emits_partial_then_final_on_speech_then_silence() {
        let (mut producer, consumer) = create_audio_ring();
        producer.push_slice(&vec![0.2; 960]);
        producer.push_slice(&vec![0.0; 960]);

        let calls = Arc::new(Mutex::new(Vec::new()));
        let model_resets = Arc::new(AtomicUsize::new(0));
        let model = ModelHandle::new(TestModel {
            calls: Arc::clone(&calls),
            resets: Arc::clone(&model_resets),
            empty_partial: false,
            empty_final: false,
            fail_final: false,
        });

        let vad_resets = Arc::new(AtomicUsize::new(0));
        let vad: Box<dyn VoiceActivityDetector> = Box::new(ScriptedVad::new(
            vec![VadDecision::Speech, VadDecision::Silence],
            Arc::clone(&vad_resets),
        ));

        let (transcript_tx, mut transcript_rx) = broadcast::channel(16);
        let (status_tx, _) = broadcast::channel(8);
        let (activity_tx, _) = broadcast::channel(8);
        let running = Arc::new(AtomicBool::new(true));

        let ctx = PipelineContext {
            config: base_config(),
            model,
            vad,
            consumer,
            running: Arc::clone(&running),
            transcript_tx,
            status_tx,
            activity_tx,
            status: Arc::new(Mutex::new(EngineStatus::Idle)),
            seq: Arc::new(AtomicU64::new(0)),
            capture_sample_rate: 16_000,
            diagnostics: Arc::new(PipelineDiagnostics::default()),
        };

        let handle = thread::spawn(move || run(ctx));

        let first = recv_event_with_timeout(&mut transcript_rx, Duration::from_secs(1));
        let second = recv_event_with_timeout(&mut transcript_rx, Duration::from_secs(1));

        running.store(false, Ordering::SeqCst);
        handle.join().expect("pipeline thread panicked");

        assert_eq!(first.segments[0].kind, SegmentKind::Partial);
        assert_eq!(second.segments[0].kind, SegmentKind::Final);
        assert_eq!(first.segments[0].id, second.segments[0].id);
        assert_eq!(&*calls.lock(), &vec![true, false]);
        assert_eq!(vad_resets.load(Ordering::Relaxed), 1);
        assert_eq!(model_resets.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn run_forces_final_flush_when_max_speech_samples_reached() {
        let (mut producer, consumer) = create_audio_ring();
        producer.push_slice(&vec![0.3; 960]);

        let calls = Arc::new(Mutex::new(Vec::new()));
        let model_resets = Arc::new(AtomicUsize::new(0));
        let model = ModelHandle::new(TestModel {
            calls: Arc::clone(&calls),
            resets: Arc::clone(&model_resets),
            empty_partial: false,
            empty_final: false,
            fail_final: false,
        });

        let vad_resets = Arc::new(AtomicUsize::new(0));
        let vad: Box<dyn VoiceActivityDetector> = Box::new(ScriptedVad::new(
            vec![VadDecision::Speech],
            Arc::clone(&vad_resets),
        ));

        let (transcript_tx, mut transcript_rx) = broadcast::channel(16);
        let (status_tx, _) = broadcast::channel(8);
        let (activity_tx, _) = broadcast::channel(8);
        let running = Arc::new(AtomicBool::new(true));

        let mut cfg = base_config();
        cfg.min_speech_samples = 4_000;
        cfg.max_speech_samples = 960;

        let ctx = PipelineContext {
            config: cfg,
            model,
            vad,
            consumer,
            running: Arc::clone(&running),
            transcript_tx,
            status_tx,
            activity_tx,
            status: Arc::new(Mutex::new(EngineStatus::Idle)),
            seq: Arc::new(AtomicU64::new(0)),
            capture_sample_rate: 16_000,
            diagnostics: Arc::new(PipelineDiagnostics::default()),
        };

        let handle = thread::spawn(move || run(ctx));
        let event = recv_event_with_timeout(&mut transcript_rx, Duration::from_secs(1));
        running.store(false, Ordering::SeqCst);
        handle.join().expect("pipeline thread panicked");

        assert_eq!(event.segments[0].kind, SegmentKind::Final);
        assert_eq!(event.segments[0].id, "utt-0");
        assert_eq!(&*calls.lock(), &vec![false]);
        assert_eq!(vad_resets.load(Ordering::Relaxed), 1);
        assert_eq!(model_resets.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn run_forces_final_flush_on_stop_with_buffered_speech() {
        let (mut producer, consumer) = create_audio_ring();
        producer.push_slice(&vec![0.3; 960]);

        let calls = Arc::new(Mutex::new(Vec::new()));
        let model_resets = Arc::new(AtomicUsize::new(0));
        let model = ModelHandle::new(TestModel {
            calls: Arc::clone(&calls),
            resets: Arc::clone(&model_resets),
            empty_partial: false,
            empty_final: false,
            fail_final: false,
        });

        let vad_resets = Arc::new(AtomicUsize::new(0));
        let vad: Box<dyn VoiceActivityDetector> = Box::new(ScriptedVad::new(
            vec![VadDecision::Speech],
            Arc::clone(&vad_resets),
        ));

        let (transcript_tx, mut transcript_rx) = broadcast::channel(16);
        let (status_tx, _) = broadcast::channel(8);
        let (activity_tx, _) = broadcast::channel(8);
        let running = Arc::new(AtomicBool::new(true));

        let mut cfg = base_config();
        cfg.min_speech_samples = 960;
        cfg.max_speech_samples = 8_000;

        let ctx = PipelineContext {
            config: cfg,
            model,
            vad,
            consumer,
            running: Arc::clone(&running),
            transcript_tx,
            status_tx,
            activity_tx,
            status: Arc::new(Mutex::new(EngineStatus::Idle)),
            seq: Arc::new(AtomicU64::new(0)),
            capture_sample_rate: 16_000,
            diagnostics: Arc::new(PipelineDiagnostics::default()),
        };

        let handle = thread::spawn(move || run(ctx));

        // Let one speech chunk process, then stop without a silence frame.
        std::thread::sleep(Duration::from_millis(30));
        running.store(false, Ordering::SeqCst);
        handle.join().expect("pipeline thread panicked");

        let first = recv_event_with_timeout(&mut transcript_rx, Duration::from_secs(1));
        let second = recv_event_with_timeout(&mut transcript_rx, Duration::from_secs(1));

        assert_eq!(first.segments[0].kind, SegmentKind::Partial);
        assert_eq!(second.segments[0].kind, SegmentKind::Final);
        assert_eq!(first.segments[0].id, second.segments[0].id);
        assert_eq!(&*calls.lock(), &vec![true, false]);
        assert_eq!(vad_resets.load(Ordering::Relaxed), 1);
        assert_eq!(model_resets.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn run_emits_fallback_segment_when_final_is_empty() {
        let (mut producer, consumer) = create_audio_ring();
        producer.push_slice(&vec![0.3; 960]);

        let calls = Arc::new(Mutex::new(Vec::new()));
        let model_resets = Arc::new(AtomicUsize::new(0));
        let model = ModelHandle::new(TestModel {
            calls,
            resets: Arc::clone(&model_resets),
            empty_partial: false,
            empty_final: true,
            fail_final: false,
        });

        let vad_resets = Arc::new(AtomicUsize::new(0));
        let vad: Box<dyn VoiceActivityDetector> = Box::new(ScriptedVad::new(
            vec![VadDecision::Speech],
            Arc::clone(&vad_resets),
        ));

        let (transcript_tx, mut transcript_rx) = broadcast::channel(16);
        let (status_tx, _) = broadcast::channel(8);
        let (activity_tx, _) = broadcast::channel(8);
        let running = Arc::new(AtomicBool::new(true));

        let mut cfg = base_config();
        cfg.min_speech_samples = 960;
        cfg.max_speech_samples = 8_000;

        let ctx = PipelineContext {
            config: cfg,
            model,
            vad,
            consumer,
            running: Arc::clone(&running),
            transcript_tx,
            status_tx,
            activity_tx,
            status: Arc::new(Mutex::new(EngineStatus::Idle)),
            seq: Arc::new(AtomicU64::new(0)),
            capture_sample_rate: 16_000,
            diagnostics: Arc::new(PipelineDiagnostics::default()),
        };

        let handle = thread::spawn(move || run(ctx));
        std::thread::sleep(Duration::from_millis(30));
        running.store(false, Ordering::SeqCst);
        handle.join().expect("pipeline thread panicked");

        let first = recv_event_with_timeout(&mut transcript_rx, Duration::from_secs(1));
        let second = recv_event_with_timeout(&mut transcript_rx, Duration::from_secs(1));
        assert_eq!(first.segments[0].kind, SegmentKind::Partial);
        assert_eq!(second.segments[0].kind, SegmentKind::Final);
        assert_eq!(second.segments[0].text, FALLBACK_TEXT);
        assert_eq!(vad_resets.load(Ordering::Relaxed), 1);
        assert_eq!(model_resets.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn run_rescues_final_inference_when_only_rms_activity_seen_without_vad_speech() {
        let (mut producer, consumer) = create_audio_ring();
        producer.push_slice(&vec![0.02; 960]);

        let calls = Arc::new(Mutex::new(Vec::new()));
        let model_resets = Arc::new(AtomicUsize::new(0));
        let model = ModelHandle::new(TestModel {
            calls: Arc::clone(&calls),
            resets: Arc::clone(&model_resets),
            empty_partial: false,
            empty_final: false,
            fail_final: false,
        });

        let vad_resets = Arc::new(AtomicUsize::new(0));
        let vad: Box<dyn VoiceActivityDetector> = Box::new(ScriptedVad::new(
            vec![VadDecision::Silence],
            Arc::clone(&vad_resets),
        ));

        let (transcript_tx, mut transcript_rx) = broadcast::channel(16);
        let (status_tx, _) = broadcast::channel(8);
        let (activity_tx, _) = broadcast::channel(8);
        let running = Arc::new(AtomicBool::new(true));

        let mut cfg = base_config();
        cfg.vad_threshold = 0.01;
        cfg.min_speech_samples = 960;
        cfg.max_speech_samples = 8_000;

        let ctx = PipelineContext {
            config: cfg,
            model,
            vad,
            consumer,
            running: Arc::clone(&running),
            transcript_tx,
            status_tx,
            activity_tx,
            status: Arc::new(Mutex::new(EngineStatus::Idle)),
            seq: Arc::new(AtomicU64::new(0)),
            capture_sample_rate: 16_000,
            diagnostics: Arc::new(PipelineDiagnostics::default()),
        };

        let handle = thread::spawn(move || run(ctx));
        std::thread::sleep(Duration::from_millis(30));
        running.store(false, Ordering::SeqCst);
        handle.join().expect("pipeline thread panicked");

        let event = recv_event_with_timeout(&mut transcript_rx, Duration::from_secs(1));
        assert_eq!(event.segments[0].kind, SegmentKind::Final);
        assert_eq!(event.segments[0].text, "final");
        assert_eq!(&*calls.lock(), &vec![false]);
        assert_eq!(vad_resets.load(Ordering::Relaxed), 0);
        assert_eq!(model_resets.load(Ordering::Relaxed), 0);
    }
}
