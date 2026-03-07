# Dictum Audit - March 2026

## Scope

This audit reviews Dictum as a Windows-first, local-first dictation product with reliability and performance prioritized over visual redesign. The code review covered `dictum-core`, `dictum-app`, `dictum-ui`, shared IPC types, local storage, and the GitHub release workflow.

## Evidence Reviewed

- `dictum-core/src/engine/pipeline.rs`
- `dictum-core/src/inference/onnx.rs`
- `dictum-core/src/audio/device.rs`
- `dictum-core/src/bin/benchmark.rs`
- `dictum-core/tests/pipeline_latency.rs`
- `dictum-app/src/main.rs`
- `dictum-app/src/commands.rs`
- `dictum-app/src/settings.rs`
- `dictum-app/src/state.rs`
- `dictum-app/src/storage.rs`
- `dictum-app/src/text_injector.rs`
- `dictum-ui/src/app/page.tsx`
- `dictum-ui/src/hooks/useEngine.ts`
- `dictum-ui/src/hooks/useTranscript.ts`
- `dictum-ui/src/hooks/useActivity.ts`
- `.github/workflows/release-windows.yml`
- `README.md`
- `plan.md`

Validation baseline used during this audit:

- `cargo check`
- `cargo test -p dictum-core`
- `npm run typecheck`
- `npm run build`

Observed build caveat:

- Next build still emits a `Newsreader` font override warning from `dictum-ui/src/app/layout.tsx`.

## Executive Summary

Dictum's core dictation pipeline is materially stronger than the product edges around it. The engine already has practical fallback logic, duplicate suppression, stage latency tracking for the post-inference path, and a benchmark-driven tuning surface. The largest current risks are not the pill UI or the transcription core; they are release/update reliability, history scalability, incomplete observability, and workflow density in the main app surface.

The next two releases should avoid a backend rewrite unless fixture-backed benchmarks show ONNX tuning cannot meet the target. There is still meaningful headroom in the existing stack through instrumentation, storage/query fixes, better diagnostics export, and context-aware workflow improvements.

## Ranked Findings

| Priority | Finding | User Impact | Evidence | Estimate |
| --- | --- | --- | --- | --- |
| P1 | History search and stats do full-row processing in app code instead of indexed query paths. | Large history libraries will feel slow, pagination will degrade, and stats/history tabs will compete with UI responsiveness. | `dictum-app/src/storage.rs:293`, `dictum-app/src/storage.rs:390` | M |
| P1 | Release/update reliability has been brittle in live shipping paths. | Users cannot trust in-app updates if repo slug, signing assumptions, or asset expectations drift again. | `dictum-app/src/commands.rs:25`, `dictum-app/src/commands.rs:353`, `.github/workflows/release-windows.yml:106` | M |
| P1 | Observability stops at finalize/inject/persist; capture, VAD, and inference latency are not directly measured per session. | Performance tuning remains guess-heavy, and regressions in the most expensive stages can hide behind aggregate finalize numbers. | `dictum-app/src/state.rs:76`, `dictum-app/src/main.rs:519`, `dictum-core/src/engine/pipeline.rs:38` | M |
| P1 | The main UI surface is a single 2.5k-line page with product logic, updater state, calibration, onboarding, history, and settings all mixed together. | Regression risk is high, review velocity drops, and workflow-specific fixes become harder to land safely. | `dictum-ui/src/app/page.tsx` is 2494 lines | M |
| P2 | Settings and runtime environment propagation are duplicated across startup, auto-tune, and runtime update flows. | Configuration drift is more likely during future feature work, especially with model/profile/cloud flags. | `dictum-app/src/settings.rs:328`, `dictum-app/src/commands.rs:590`, `dictum-app/src/commands.rs:863` | S |
| P2 | Audio device recommendation is still name-heuristic only. | Wrong default mic selection remains likely on complex Windows setups with virtual devices, docks, USB headsets, or vendor-renamed inputs. | `dictum-core/src/audio/device.rs:15` | S |
| P2 | History encryption is path and machine derived, not backed by an OS secret store. | The current "encrypted history" story is weak for privacy-sensitive users and can complicate migration/portability semantics. | `dictum-app/src/storage.rs:95` | M |
| P2 | Benchmark CLI exists, but the default fixture directory is not committed and there is no standard hardware baseline in repo. | Reliability and tuning decisions still depend too much on ad hoc local testing. | `dictum-core/src/bin/benchmark.rs:96`, missing `benchmarks/fixtures/` directory | M |
| P3 | Legacy branding and path conventions remain in persisted storage/settings roots. | Migration and support become harder because the product identity and storage path history are now mixed. | `dictum-app/src/storage.rs:163`, `dictum-app/src/settings.rs:412` | S |
| P3 | Font/build warning remains in the UI layer. | Low user impact, but it adds noise to release verification and can hide more important build warnings. | `dictum-ui/src/app/layout.tsx:5` | XS |

## Core Speech Pipeline Audit

### What looks healthy

- No major issue found in stop/start race protection on the frontend. `useEngine` serializes start/stop operations and treats "already running" and "not running" as idempotent states rather than hard failures in `dictum-ui/src/hooks/useEngine.ts`.
- No major issue found in final transcript handling. The app applies learned corrections, dictionary/snippet transforms, duplicate suppression, placeholder fallback skipping, and history persistence in a coherent finalization path in `dictum-app/src/main.rs:475`.
- No major issue found in fallback coverage. The pipeline has tests for empty-final fallback and RMS rescue behavior in `dictum-core/src/engine/pipeline.rs:1115` and `dictum-core/src/engine/pipeline.rs:1174`.
- No major issue found in time-to-first-partial coverage. `dictum-core/tests/pipeline_latency.rs:85` asserts the first transcript event arrives in under 500 ms for the synthetic latency test path.

### Latency Budget

The product does not yet expose a full measured capture-to-inject latency budget. Current numbers are a mix of measured and inferred stages:

| Stage | Current State | Evidence | Audit Read |
| --- | --- | --- | --- |
| Capture drain cadence | Inferred | `dictum-core/src/engine/pipeline.rs:112` uses 20 ms drain chunks, `empty_sleep_ms()` defaults to 5 ms | Good baseline for low idle CPU without obvious busy-spin risk |
| Resample + VAD | Inferred | `dictum-core/src/engine/pipeline.rs:194`, `dictum-core/src/engine/pipeline.rs:223` | Present, but no dedicated latency telemetry |
| Partial time-to-first-word | Measured by test | `dictum-core/tests/pipeline_latency.rs:85` | Guard exists, but only for synthetic delay path |
| Inference | Inferred / benchmark-only | `dictum-core/src/bin/benchmark.rs`, `dictum-core/src/engine/pipeline.rs:486` | Biggest blind spot for live tuning |
| Transform | Measured | `dictum-app/src/main.rs:499`, `dictum-app/src/state.rs:137` | Healthy |
| Finalize wrapper | Measured | `dictum-app/src/main.rs:621`, `dictum-app/src/state.rs:149` | Useful but too coarse for engine diagnosis |
| Inject | Measured | `dictum-app/src/main.rs:592`, `dictum-app/src/state.rs:141` | Healthy |
| Persist | Measured | `dictum-app/src/main.rs:614`, `dictum-app/src/state.rs:145` | Healthy |

Recommended target budget for the next milestone:

- Capture/resample/VAD overhead: under 40 ms p95 on steady speech
- Inference: expose separate p50/p95 by model profile and execution provider
- Transform + finalize wrapper: under 20 ms p95
- Inject: under 35 ms p95 for normal text paths
- Persist: under 15 ms p95
- End-to-end finalization: reduce current observed p95 by 25-40 percent, matching `plan.md`

### Failure-Mode Matrix

| Scenario | Current Handling | Evidence | Residual Risk | Next Step |
| --- | --- | --- | --- | --- |
| Quiet mic / whisper speech | Whisper-biased performance profile, adaptive gain, activity tuning, RMS rescue fallback | `dictum-app/src/settings.rs:167`, `dictum-core/src/engine/pipeline.rs:420` | Hard to tune confidently without fixture pack and per-stage metrics | Build benchmark fixture pack and store baseline reports |
| Empty final transcript | Empty-final streak tracking and fallback placeholder emission | `dictum-core/src/engine/pipeline.rs:574` | Placeholder still degrades UX if partial rescue is unavailable | Add diagnostics for empty-final reason distribution |
| Placeholder final after useful partial | Recent partial can rescue placeholder final before injection | `dictum-app/src/main.rs:528` | Rescue window is heuristic and may miss long pauses | Add "final rescued from partial" metric to diagnostics export |
| Duplicate injection | Recent final dedupe window blocks near-identical duplicate finals | `dictum-app/src/main.rs:566` | Very short dedupe window may still allow multi-second duplicates | Track duplicate suppression count and widen only if fixture evidence supports it |
| Rapid stop before silence | Forced terminal final flush on stop | `dictum-core/src/engine/pipeline.rs:379` | Good behavior, but no explicit regression matrix in CI | Add stop/start fixture case to benchmark test plan |
| RMS activity without VAD speech | Rescue inference from rolling buffer, then fallback | `dictum-core/src/engine/pipeline.rs:420` | Indicates VAD misses may still be happening in edge conditions | Expose RMS-rescue counts in perf export and onboarding diagnostics |
| Low-confidence local decode | Reliability mode re-decodes and may escalate to cloud fallback | `dictum-core/src/inference/onnx.rs:1167` | Heuristic confidence formula may be unstable across profiles | Validate thresholds against fixture pack before broadening fallback behavior |
| Cloud fallback missing API key / unavailable | Falls back to local-only output path or Windows dictation fallback | `dictum-core/src/inference/onnx.rs:1201`, `dictum-core/src/inference/onnx.rs:1653` | Failure reasons are not visible enough to users | Surface fallback reason in diagnostics and settings |
| Noisy room / clipping | Activity noise gate, clip threshold, auto-tune inputs | `dictum-ui/src/app/page.tsx:189`, `dictum-app/src/commands.rs:664` | Calibration is useful but still fairly opaque | Keep UI as-is visually, but add clearer measured recommendations |
| Long-form utterance | Forced flush with tail retention overlap | `dictum-core/src/engine/pipeline.rs:291` | Heuristic overlap may duplicate or fragment long speech on some profiles | Benchmark long-form fixtures before backend work |

### Low-Risk Tuning

- Add measured stage timings for resample, VAD classify, and model transcribe to extend `PerfSnapshot`.
- Add counters for duplicate suppression, partial rescue use, cloud fallback attempted/succeeded, and Windows fallback attempted/succeeded.
- Replace in-memory history query filtering with indexed metadata filtering and explicit search strategy.
- Centralize runtime env synchronization so startup, `set_runtime_settings`, and auto-tune all use one code path.
- Add fixture-backed benchmark reports per model profile and execution provider before changing defaults again.
- Record mic/device identifiers and success rates for recommended-device selection, without changing the UI surface.

### Structural Changes

- Add a background diagnostics bundle exporter that captures perf snapshot, fallback reasons, update telemetry, and storage stats.
- Split the main page into workflow-specific modules while preserving the current design language.
- Introduce per-app profiles with a real persisted schema, not just UI-only state.
- Consider a backend alternative such as Faster-Whisper only if benchmark data shows ONNX tuning still misses target on CPU-only or noisy/quiet edge cases.

## Desktop Runtime And Operations Audit

### Findings

- Update checks now normalize legacy repo slugs, but the recent 404 failure proved this path is fragile enough to justify explicit release smoke testing. See `dictum-app/src/commands.rs:91`.
- Installer download is already checksum validated and signature checked, which is a strong foundation. See `dictum-app/src/commands.rs:469` and `dictum-app/src/commands.rs:231`.
- Release workflow now writes `SHA256SUMS.txt` and `RELEASE_MANIFEST.json`, but recent certificate import and subject validation failures show that "workflow passed once" is not enough evidence for ongoing release trust. See `.github/workflows/release-windows.yml:106`.
- Single-instance enforcement is present and pragmatic for Windows. See `dictum-app/src/main.rs:54`.
- Settings persistence is functionally sound, but there is path/branding drift and duplicated env propagation logic. See `dictum-app/src/settings.rs:328` and `dictum-app/src/settings.rs:412`.
- Local storage is WAL-backed and indexed by timestamp, but history search and stats queries do not scale with encrypted content volume. See `dictum-app/src/storage.rs:179`, `dictum-app/src/storage.rs:293`, and `dictum-app/src/storage.rs:390`.

### Release-Hardening Checklist

- Run a canary tag through the GitHub workflow after any signing or release-script change.
- Verify the normalized update repo slug in both frontend and backend defaults before each GA release.
- Confirm the release contains installer, portable exe, `SHA256SUMS.txt`, and `RELEASE_MANIFEST.json`.
- Confirm `SHA256SUMS.txt` contains both installer and exe entries.
- Verify Authenticode status on the installer artifact after upload, not just before release creation.
- Smoke-test update flow from the previous public installer, not only from a dev build.
- Smoke-test three negative paths: missing checksum asset, bad checksum value, and invalid repo slug.
- Publish rollback instructions in the release doc for one previous version.
- Treat all nonzero workflow warnings as release blockers until the `Newsreader` warning is resolved or explicitly waived.

### Risk Register

| Risk | Likelihood | Impact | Evidence | Mitigation |
| --- | --- | --- | --- | --- |
| Update repo drift breaks release checks again | Medium | High | live 404 issue, slug normalization in `dictum-app/src/commands.rs:91` | Keep one canonical release config source and add release smoke test |
| Signing workflow regresses with certificate assumptions | Medium | High | recent workflow fixes in `.github/workflows/release-windows.yml:106` | Keep canary workflow and signature verification gate |
| Settings/env drift after new flags are added | Medium | Medium | duplicated env writes across `settings.rs` and `commands.rs` | Centralize runtime env application helper |
| History/storage slows materially for active users | High | Medium | `get_history` and `get_stats` scan/decrypt in app | Add indexed metadata and narrower query strategy |
| Privacy expectations exceed actual history protection | Medium | Medium | key derived from username/computer/path in `storage.rs:95` | Move to DPAPI or Windows Credential Manager backed secret |
| Future schema changes break old installs | Medium | Medium | settings/history paths still use legacy vendor roots | Add explicit migration versioning and one-time path migration |

### Local Monitoring And Diagnostic Export Proposal

Recommended additive capability for `v0.1.7`:

- New Tauri command: `export_diagnostics_bundle(path?: string)` or `build_diagnostics_bundle() -> DiagnosticsBundle`
- Scope:
  - Perf snapshot with stage timings and pipeline counters
  - Fallback reason counts
  - Update telemetry log already collected in the UI
  - Runtime settings safe subset
  - Model profile, ORT provider, and thread config
  - History volume stats and prune stats
  - Device list with recommended/default markers
  - App version, release manifest info, and updater repo slug
- Format:
  - JSON for machine-readable export
  - Optional ZIP if screenshots/log files are added later

Why this is worth doing:

- It reduces bug report ambiguity.
- It makes auto-tune outcomes auditable.
- It gives the benchmark/fixture work a place to land in product-facing diagnostics.

## Secondary UX And Workflow Audit

### Friction Points

- The settings/live/update/calibration/history surfaces are packed into one page component, which makes the product feel denser than it needs to even without a visual redesign.
- History search is live, but the backend work done for search is expensive enough that the UX will eventually degrade with real data volume.
- Onboarding, model recommendation, auto-tune, privacy, and update controls all live in the same conceptual area, which makes the "what should I do first?" story weaker than it should be.
- Update behavior now has multiple local storage flags and telemetry states in the UI, but there is no compact "updater health" summary for the user.
- Correction learning exists, but its current workflow still feels reactive rather than explicit; there is not yet a strong user-facing correction memory mental model.

### Non-Visual UX Improvements With Measurable Value

- Add a compact diagnostics summary card in Settings showing provider, model, finalize p95, fallback rate, and update repo. No redesign required.
- Add clear post-auto-tune output that explains what changed and whether Dictum recommends re-running tune after a hardware or mic change.
- Add empty-state guidance in History when history is disabled or when search is slow because the dataset is large.
- Add one-click export for diagnostics bundle so support and regression tracking stop relying on screenshots and manual copy/paste.
- Add explicit fallback reason text in the live status area when cloud fallback or Windows fallback is used.

### Nice-To-Have UI Polish Appendix

- Resolve the `Newsreader` build warning.
- Break the settings tab into smaller sections with sticky headings, but preserve the current visual language.
- Add keyboard shortcuts/help discovery text for update, tuning, and correction workflows.

## Feature Expansion Scan

### Feature Matrix

Scoring uses 1-5 where higher user value is better, and lower cost/risk is better.

| Feature | User Value | Cost | Risk | Dependency Chain | Recommendation |
| --- | --- | --- | --- | --- | --- |
| Per-app profiles | 5 | 3 | 3 | Foreground app detection, settings schema, transform/runtime profile binding | Ship in `v0.1.8` |
| Dictation modes (`coding`, `command`, `conversation`) | 4 | 3 | 2 | Per-app profile groundwork, post-processing presets | Ship in `v0.1.8` if profile scaffolding lands |
| Correction memory v2 with better review/teaching | 5 | 2 | 2 | Existing learned correction store already present | Ship in `v0.1.8` |
| Phrase bias manager with import/export | 4 | 2 | 2 | Existing phrase bias env path in ONNX code | Start in `v0.1.8`, can split |
| Benchmark fixture pack and baseline reports | 5 | 2 | 1 | Existing benchmark CLI | Ship in `v0.1.7` |
| Diagnostics bundle export | 5 | 2 | 1 | Existing perf snapshot and update telemetry | Ship in `v0.1.7` |
| History indexing/search redesign | 4 | 3 | 2 | Storage schema extension | Ship in `v0.1.7` |
| Faster-Whisper prototype | 3 | 4 | 4 | Benchmark baselines first | Investigate later |
| whisper.cpp backend | 2 | 4 | 4 | Same as above | Investigate later |
| Optional encrypted settings sync | 2 | 4 | 4 | Security model, account/storage work | Defer |

### Recommendation

Place these in the next two releases:

- `v0.1.7`: diagnostics bundle export, benchmark fixture pack + baseline reports, history/search performance work, updater/release hardening, runtime env/config consolidation.
- `v0.1.8`: per-app profiles, dictation modes, correction memory v2, phrase bias manager improvements.

Defer backend swaps until the benchmark fixture pack proves the existing ONNX path cannot meet latency/reliability targets after tuning.

## Downstream Interface Changes To Plan For

No breaking API or IPC changes are required for the audit itself. The likely downstream changes are mostly additive.

| Area | Proposed Change | Additive Or Breaking | Migration | Persisted Data Impact |
| --- | --- | --- | --- | --- |
| Tauri IPC | `get_diagnostics_bundle_preview` or `export_diagnostics_bundle` | Additive | None | None |
| Tauri IPC | richer `PerfSnapshot` with `captureMs`, `vadMs`, `inferenceMs`, `fallbackStats` | Additive | UI needs to tolerate absent fields for old binaries if mixed versions matter | None |
| Settings schema | per-app profiles, dictation mode presets, fallback preference detail | Additive if versioned carefully | one-time default profile creation | Yes, settings JSON |
| Storage schema | searchable history metadata table or FTS sidecar for plaintext-normalized terms | Additive schema migration | one-time backfill/index build | Yes, history DB |
| Storage schema | correction metadata richness such as per-language or confidence counters | Additive | lightweight backfill or default columns | Yes, settings or DB |

## Decision Log

### Keep

- Keep the current ONNX backend for the next milestone unless fixture-backed evidence disproves it.
- Keep duplicate suppression and final transcript post-processing in the app layer.
- Keep the current visual design direction, including the pill, for this workstream.

### Change

- Change storage/query strategy for history and stats.
- Change release verification from "workflow exists" to "workflow plus smoke-tested updater path."
- Change diagnostics from coarse post-inference timing to full speech-pipeline timing.
- Change the main page implementation structure without redesigning the UI language.

### Investigate Later

- Faster-Whisper and whisper.cpp as optional backends.
- Optional sync/export of settings and correction memory across devices.
- More advanced confidence UI once benchmark-backed confidence quality is validated.

## Recommended Next Actions

1. Land `v0.1.7` work around diagnostics, history performance, release hardening, and benchmark fixtures before adding new dictation features.
2. Use the fixture pack to decide whether the current ONNX stack can stay as the only backend through `v0.1.8`.
3. Land per-app profiles and dictation modes only after the settings/storage foundations are ready.
