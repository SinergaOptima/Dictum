# Dictum — Transcription Recovery + Optimization Plan
**Lattice Labs | Rust + Tauri + Next.js**

_Last updated: 2026-02-19_

---

## 1) Mission (Current Priority)

Dictum currently captures audio and responds to hotkeys, but transcription is not reliably producing typed text in real-world usage.

The immediate objective is to make transcription production-grade reliable on Windows:

1. Audio is captured from the selected microphone.
2. Speech is detected and flushed into inference.
3. Whisper ONNX returns non-empty transcript segments.
4. Final segments are emitted through Tauri events.
5. Final text is typed into the active app (global dictation behavior).

No additional UI polish work should preempt this path until these pass gates are green.

---

## 2) Definition Of Done (Hard Exit Criteria)

All of the following must pass before calling this fixed:

1. **Manual E2E pass (Windows)**  
Speak 10 short phrases into Notepad with `Ctrl+Shift+Space`; at least 9/10 phrases appear as typed text in the focused app.
2. **Mic routing pass**  
Changing mic in UI affects subsequent capture immediately after restart and after global shortcut start.
3. **Pipeline observability pass**  
For each utterance: we can see logs for `audio -> vad -> inference -> transcript emit -> text inject`.
4. **Regression suite pass**  
`cargo test -p dictum-core`, `cargo check -p dictum-app`, `npm run typecheck`, `npm run build`.
5. **Latency guardrail pass**  
First token / first segment latency remains under the existing test target (`< 500 ms` in integration harness).

---

## 3) Root-Cause Workstreams (P0)

### P0-A: End-to-End Tracing + Diagnostics

| ID | Task | Files | Output |
|----|------|-------|--------|
| TR-001 | Add structured per-utterance trace ID (`utt-*`) through capture, VAD, inference, emit, injection | `dictum-core/src/engine/pipeline.rs`, `dictum-app/src/main.rs` | Single utterance can be traced across every stage |
| TR-002 | Add debug counters (`frames_in`, `frames_resampled`, `vad_windows`, `inference_calls`, `segments_emitted`, `inject_calls`) | `dictum-core/src/engine/pipeline.rs`, `dictum-app/src/main.rs` | Numeric diagnostics printed at stop + per utterance |
| TR-003 | Add `DICTUM_DEBUG_TRANSCRIBE=1` mode to log decoder token stream for first 20 steps | `dictum-core/src/inference/onnx.rs` | Determine if decode is collapsing to empty/eot/noise tokens |
| TR-004 | Add explicit startup report for model files and ONNX I/O names | `dictum-core/src/inference/onnx.rs`, `dictum-core/src/vad/silero.rs` | Immediate visibility of bad model export mismatch |

### P0-B: Inference Correctness (Whisper ONNX)

| ID | Task | Files | Output |
|----|------|-------|--------|
| TR-005 | Validate mel frontend against Whisper spec (window, hop, scaling, clipping) using fixture test | `dictum-core/src/inference/onnx.rs`, `dictum-core/tests/*` | Prevent silent decode failures from frontend mismatch |
| TR-006 | Add decoder input compatibility branch: base decoder vs cached decoder formats | `dictum-core/src/inference/onnx.rs` | Works with common optimum export variants |
| TR-007 | Implement prefix strategy table (`multilingual`, `en-only`) with fallback and logging | `dictum-core/src/inference/onnx.rs` | Fewer empty text decodes |
| TR-008 | Add confidence guardrails: if token stream is empty twice, emit explicit inference warning event | `dictum-core/src/engine/pipeline.rs` | UI/user sees actionable error instead of silent failure |

### P0-C: VAD + Flush Reliability

| ID | Task | Files | Output |
|----|------|-------|--------|
| TR-009 | Add temporary bypass mode (`always_transcribe`) for isolation | `dictum-core/src/engine/mod.rs`, `dictum-core/src/engine/pipeline.rs` | Confirms whether failure is VAD-gating vs inference |
| TR-010 | Tune VAD thresholds/hangover with runtime config and live display | `dictum-core/src/engine/mod.rs`, `dictum-ui/src/app/page.tsx` | Speech segments reliably flushed |
| TR-011 | Ensure stop() forces final flush when speech buffer is non-empty | `dictum-core/src/engine/pipeline.rs` | No lost final utterance on key-up/stop |
| TR-012 | Add tests for long pause, short phrase, and stop-mid-utterance | `dictum-core/src/engine/pipeline.rs`, `dictum-core/tests/*` | Reproducible stability across edge cases |

### P0-D: IPC + UI Event Delivery

| ID | Task | Files | Output |
|----|------|-------|--------|
| TR-013 | Add transcript event watchdog in UI (last-event timestamp + health indicator) | `dictum-ui/src/hooks/useTranscript.ts`, `dictum-ui/src/app/page.tsx` | Immediate visibility if backend emits stop arriving |
| TR-014 | Add event payload validation in frontend (reject malformed segments with log) | `dictum-ui/src/lib/tauri.ts`, `dictum-ui/src/hooks/useTranscript.ts` | Prevent silent state corruption |
| TR-015 | Add backend-side emit stats and lag warnings per channel | `dictum-app/src/main.rs` | Detect channel lag / closed subscribers |

### P0-E: Global Text Injection Reliability (Windows)

| ID | Task | Files | Output |
|----|------|-------|--------|
| TR-016 | Harden `SendInput` path for UTF-16 surrogate pairs and newline policy | `dictum-app/src/text_injector.rs` | Robust typing in standard editors and browsers |
| TR-017 | Add injection mode options: `sendinput`, `clipboard-paste`, `off` | `dictum-app/src/state.rs`, `dictum-app/src/commands.rs`, `dictum-ui/src/app/page.tsx` | Fast fallback when target app blocks key injection |
| TR-018 | Add focused-window diagnostics when injection fails | `dictum-app/src/text_injector.rs`, `dictum-app/src/main.rs` | Better root-cause data for app-specific failures |

### P0-F: Test Harness + Repro

| ID | Task | Files | Output |
|----|------|-------|--------|
| TR-019 | Add deterministic WAV fixture runner for inference-only tests | `dictum-core/tests/*`, `scripts/*` | Repeatable decode checks independent of microphone |
| TR-020 | Add hardware-in-loop smoke script (record + transcribe + assert non-empty final) | `scripts/*`, `dictum-app/*` | One-command sanity test before releases |
| TR-021 | Add CI job for inference fixture tests (non-realtime) | `.github/workflows/ci.yml` | Catch regressions before merge |

---

## 4) Execution Sequence (Strict Order)

1. **Observability first**: complete TR-001 to TR-004 before changing behavior.
2. **Inference correctness second**: complete TR-005 to TR-008.
3. **VAD and flush path third**: complete TR-009 to TR-012.
4. **IPC/UI verification fourth**: complete TR-013 to TR-015.
5. **Injection hardening fifth**: complete TR-016 to TR-018.
6. **Automated reproducibility sixth**: complete TR-019 to TR-021.

If any step fails, pause downstream work and fix the failing stage before moving forward.

---

## 5) Validation Matrix (What We Must Test Every Iteration)

| Scenario | Input | Expected Result |
|----------|-------|-----------------|
| S1 | Default mic, short sentence, toggle start/stop | At least one final segment + typed text appears |
| S2 | Non-default mic selected in UI | Transcript source reflects selected device; typed output appears |
| S3 | Speak continuously 10+ seconds | Partial updates + final segment at pause |
| S4 | Start speaking, stop mid-sentence | Forced final flush emits non-empty text |
| S5 | Background app target (Notepad/VSCode/browser textbox) | Text injection works in focused control |
| S6 | No speech noise only | No hallucinated long transcript; pipeline remains stable |
| S7 | Model missing/corrupt | Clear error state surfaced in UI and logs |

---

## 6) Performance Optimization Path (After Functional Stability)

### P1 Optimization (Latency + Throughput)

| ID | Task | Goal |
|----|------|------|
| OP-001 | Add per-stage timing spans in pipeline | Pinpoint slowest stage for first-word latency |
| OP-002 | Reduce unnecessary allocations in mel + decode loops | Lower CPU/memory churn |
| OP-003 | Evaluate chunk sizing and flush cadence | Better responsiveness without transcript fragmentation |
| OP-004 | Add CPU thread affinity/options for inference task | More stable latency under desktop load |

### P2 Optimization (Model Runtime)

| ID | Task | Goal |
|----|------|------|
| OP-005 | Add DirectML execution provider option | GPU acceleration on Windows devices |
| OP-006 | Add optional quantized model variant support | Faster inference on lower-end hardware |
| OP-007 | Add model compatibility checker in startup | Prevent bad exports from silently running |

### P3 Optimization (Product Robustness)

| ID | Task | Goal |
|----|------|------|
| OP-008 | Auto-recovery after transient stream/inference errors | Fewer manual restarts |
| OP-009 | Device hot-plug handling | Robust USB mic workflows |
| OP-010 | External diagnostics bundle export | Faster issue triage and supportability |

---

## 7) Instrumentation Spec (Minimum Required Fields)

Every utterance-level diagnostic event should include:

1. `utterance_id`
2. `capture_sample_rate`
3. `target_sample_rate`
4. `raw_samples`
5. `resampled_samples`
6. `vad_decision`
7. `speech_buf_len`
8. `inference_partial_or_final`
9. `decoder_steps`
10. `decoded_text_len`
11. `segments_count`
12. `emit_success`
13. `inject_mode`
14. `inject_success`
15. `elapsed_ms_stage`

---

## 8) Risks + Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Model export mismatch (I/O names or topology) | Zero transcripts despite healthy audio/VAD | Startup compatibility checks + fail-fast errors |
| Over-aggressive VAD threshold | Speech never flushed to inference | Runtime tuning + bypass mode + tests |
| Decoder prefix/token mismatch | Empty or junk outputs | Prefix strategy fallback + token logging |
| Injection blocked in target app | Transcript visible in UI but not typed | Injection mode fallback (`clipboard-paste`) |
| Silent regressions from UI/event changes | Works locally, breaks in integration | CI fixture tests + watchdog signals |

---

## 9) Immediate 72-Hour Sprint Plan

### Day 1
1. TR-001, TR-002, TR-003, TR-004
2. Produce one real utterance trace from mic input to injection attempt

### Day 2
1. TR-005, TR-006, TR-007, TR-009, TR-011
2. Verify non-empty final segments on at least 3 manual scenarios (S1, S2, S4)

### Day 3
1. TR-013, TR-015, TR-016, TR-017, TR-019
2. Run full validation matrix and publish pass/fail report in this file

---

## 10) Status Ledger

Use this ledger to track progress without ambiguity.

| ID | Status | Date | Notes |
|----|--------|------|-------|
| TR-001 | Done | 2026-02-19 | Utterance spans with `info_span!` + structured logging in `flush_inference` |
| TR-002 | Done | 2026-02-19 | Pipeline diagnostics (frames_in, frames_resampled, vad_windows, vad_speech, inference_calls, inference_errors, segments_emitted) + inject counters in app |
| TR-003 | Done | 2026-02-19 | `DICTUM_DEBUG_TRANSCRIBE=1` env var logs token stream for first 20 steps |
| TR-004 | Done | 2026-02-19 | ONNX model I/O names logged at startup for encoder/decoder; SileroVad startup report |
| TR-005 | Pending | 2026-02-19 |  |
| TR-006 | Pending | 2026-02-19 |  |
| TR-007 | Pending | 2026-02-19 |  |
| TR-008 | Pending | 2026-02-19 |  |
| TR-009 | Pending | 2026-02-19 |  |
| TR-010 | Pending | 2026-02-19 |  |
| TR-011 | Pending | 2026-02-19 |  |
| TR-012 | Pending | 2026-02-19 |  |
| TR-013 | Pending | 2026-02-19 |  |
| TR-014 | Pending | 2026-02-19 |  |
| TR-015 | Pending | 2026-02-19 |  |
| TR-016 | Pending | 2026-02-19 |  |
| TR-017 | Pending | 2026-02-19 |  |
| TR-018 | Pending | 2026-02-19 |  |
| TR-019 | Pending | 2026-02-19 |  |
| TR-020 | Pending | 2026-02-19 |  |
| TR-021 | Pending | 2026-02-19 |  |

