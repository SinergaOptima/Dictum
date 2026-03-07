# Dictum Benchmark Fixtures

This directory holds the benchmark fixture layout and baseline reports used to tune Dictum without relying on ad hoc local testing.

## Layout

- `fixtures/quiet_speech/`
- `fixtures/whisper_speech/`
- `fixtures/noisy_room/`
- `fixtures/long_form/`
- `baselines/`

Each audio fixture should be a `.wav` file. If a reference transcript is available, place a sibling `.txt` file with the same base name.

Example:

```text
fixtures/quiet_speech/quiet_intro.wav
fixtures/quiet_speech/quiet_intro.txt
```

## Running the benchmark

From the repo root:

```powershell
cargo run -p dictum-core --features onnx --bin benchmark -- --fixtures benchmarks/fixtures --output benchmarks/baselines/local-baseline.json
```

Useful variants:

```powershell
cargo run -p dictum-core --features onnx --bin benchmark -- --fixtures benchmarks/fixtures --iterations 3
```

## Fixture guidance

- `quiet_speech`: low-volume but intelligible speech
- `whisper_speech`: whisper or near-whisper speech
- `noisy_room`: speech with realistic ambient noise
- `long_form`: longer utterances or paragraph-length speech

## Dictation mode coverage

The current smoke pack is still audio-centric, but release decisions for `0.1.8` should be framed against the three product dictation modes:

- `conversation`
  - validate natural punctuation and prose cleanup
  - check correction suggestions against plain-language phrases
- `coding`
  - validate symbol-heavy phrases, casing, and code-term biasing
  - compare correction behavior with coding-focused app profiles enabled
- `command`
  - validate lowercase command phrases, slash/dash tokens, and punctuation trimming
  - compare shell-like phrases with and without profile-specific bias terms

When adding future baselines, note which dictation mode and app-profile context were active during the run summary even if the benchmark itself remains fixture-based.

Keep committed fixtures reasonably small. If a larger private pack is used locally, keep the same category structure so reports remain comparable.

## Baselines

Commit machine-readable JSON outputs in `baselines/` when changing:

- model defaults
- ONNX threading defaults
- execution provider defaults
- fallback/confidence thresholds

Each baseline file should note the hardware and environment used to produce it.

## Supportability notes

For regression triage, pair benchmark results with a diagnostics export from the app:

1. Open Dictum and reproduce the problem case.
2. In the `Stats` tab, use `Export File` to write a diagnostics bundle locally.
3. Record the active dictation mode, matched app profile, and relevant benchmark baseline used for comparison.

This keeps perf snapshots, correction diagnostics, active-context metadata, and settings-schema health together when investigating tuning changes.

## Release gate expectations

Before cutting a public `0.1.8` release, the benchmark review should explicitly answer:

- which dictation mode the comparison was run against
- whether a matched app profile was active
- whether the diagnostics export reported any settings migration notes
- whether correction diagnostics showed orphaned or stale rules during the validation run
