# Dictum Smoke Baseline

Generated: 2026-03-06

Source report: `benchmarks/baselines/smoke-baseline.json`

## Environment

- CPU: AMD Ryzen 9 7900X (12 cores / 24 threads)
- RAM: 63.2 GiB installed
- GPU:
  - NVIDIA GeForce RTX 4070 Ti
  - AMD Radeon Graphics
- Model source: local ONNX model files from `%APPDATA%/Lattice Labs/Dictum/models`
- Command:

```powershell
cargo run -p dictum-core --features onnx --bin benchmark -- --fixtures benchmarks/fixtures --output benchmarks/baselines/smoke-baseline.json
```

## Fixture Notes

- This is a synthetic smoke set generated with Windows speech synthesis.
- `quiet_speech` and `whisper_speech` were amplitude-scaled down after synthesis.
- `noisy_room` had synthetic noise mixed into the rendered speech.
- This set is intended to catch gross latency and fallback regressions, not to represent production-quality WER benchmarking.

## Topline Results

- Total runs: 4
- p50 latency: 22765.85 ms
- p95 latency: 27069.93 ms
- Average latency: 17628.72 ms
- Miss rate: 0.0%
- Placeholder rate: 0.0%

## Interpretation

- The smoke set is suitable as a repeatable local regression check.
- The long-form and whisper-like synthetic cases matched well enough to be useful.
- The quiet and noisy synthetic cases are less realistic and should be replaced or supplemented with human-recorded fixtures for any quality-sensitive tuning work.
