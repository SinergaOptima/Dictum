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

Keep committed fixtures reasonably small. If a larger private pack is used locally, keep the same category structure so reports remain comparable.

## Baselines

Commit machine-readable JSON outputs in `baselines/` when changing:

- model defaults
- ONNX threading defaults
- execution provider defaults
- fallback/confidence thresholds

Each baseline file should note the hardware and environment used to produce it.
