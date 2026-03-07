# Dictum Roadmap - Releases 0.1.7 and 0.1.8

This roadmap converts the March 2026 audit into two shipping releases. It is intentionally execution-oriented and avoids speculative architecture work unless benchmark evidence forces it.

## Release 0.1.7 - Reliability, Observability, And Operations

### Goal

Make Dictum easier to trust under real use by improving updater reliability, making performance diagnosable, and preventing history/search degradation as usage grows.

### Scope

- Add deeper pipeline diagnostics:
  - capture/resample/VAD/inference stage timings
  - fallback reason counters
  - duplicate suppression count
  - partial rescue count
- Add diagnostics bundle export:
  - perf snapshot
  - runtime config summary
  - updater/release metadata
  - device list
  - history volume stats
- Rework history and stats data access so the app does not decrypt/filter thousands of rows before paginating.
- Consolidate runtime env application so startup, tuning, and runtime settings changes share one code path.
- Harden release/update process:
  - updater smoke-test checklist
  - negative-path validation for checksum/repo issues
  - canary validation for workflow signing changes
- Add committed benchmark fixtures and baseline JSON reports for quiet, whisper, noisy-room, and long-form scenarios.

### Acceptance Criteria

- `get_perf_snapshot` or its successor exposes measured p50/p95 timings for capture, VAD, inference, transform, finalize, inject, and persist.
- A diagnostics bundle can be exported locally without external services.
- History search for common queries remains responsive with at least 10k history entries on a normal Windows desktop.
- Stats generation no longer requires iterating every row in the selected window in Rust.
- Update checks succeed from the previous public installer using the default repo slug.
- Update install rejects bad checksums and invalid signatures with user-readable errors.
- Benchmark fixtures are committed, documented, and runnable in CI or a scripted local flow.

### Implementation Notes

- IPC changes should be additive.
- Storage changes should use a forward-only migration with backfill progress logging.
- The UI should reuse the existing settings/live surfaces rather than introducing new product areas.

## Release 0.1.8 - Context-Aware Dictation And Correction Quality

### Goal

Improve recognition usefulness for real workflows by making dictation behavior context-aware and strengthening correction memory without changing the product's visual identity.

### Dev Cadence

- `0.1.8` will ship through five internal dev builds before the public release:
  - `0.1.8-dev.1`
  - `0.1.8-dev.2`
  - `0.1.8-dev.2`
  - `0.1.8-dev.3`
  - `0.1.8-dev.4`
  - `0.1.8-dev.5`
- Public tagging and GitHub release creation should happen only after `0.1.8-dev.5` passes the release checklist.

### Scope

- Add per-app profiles:
  - map foreground app to dictation profile
  - bind profile to model/performance settings, transforms, and dictation mode
- Add dictation modes:
  - `conversation`
  - `coding`
  - `command`
- Expand correction memory:
  - clearer teach/apply model
  - higher-quality learned correction rules
  - visibility into what corrections are active
- Improve phrase bias management:
  - searchable term list
  - import/export support
  - optional per-profile term sets
- Improve settings clarity for profile/mode selection without redesigning the base UI.

### Acceptance Criteria

- Dictum can apply different runtime/profile settings automatically for at least three app contexts.
- Dictation modes measurably change post-processing behavior and are persisted in settings.
- Users can review and manage learned corrections without editing raw JSON.
- Phrase bias terms can be imported/exported and applied without restarting the app.
- `cargo test -p dictum-core`, `cargo check`, `npm run typecheck`, and `npm run build` remain green.
- `v0.1.7` diagnostics are sufficient to compare profile/mode behavior before and after rollout.

### Implementation Notes

- Start with additive settings schema fields and a default global profile.
- Avoid backend swaps in this release unless `v0.1.7` benchmark evidence shows the current stack cannot hit reliability goals.
- Keep the main page visually consistent, but split implementation into smaller modules to reduce regression risk.

### Current Dev Status

`0.1.8-dev.2` currently includes:

- guided onboarding tuning
- dictation modes
- per-app profiles
- per-app phrase bias and refinement overrides
- active profile visibility
- profile import/export
- learned correction import/export and filtering
- mode-aware correction suggestions

The next execution plans are documented in `docs/DICTUM_0.1.8_DEV2_DEV4_PLAN.md`.

## Deferred Until Proven Necessary

- Faster-Whisper integration
- whisper.cpp backend
- Optional encrypted settings sync
- Major visual redesign

## Sequencing Rules

1. Do not start backend replacement work before the fixture pack and diagnostics bundle are in place.
2. Do not ship per-app profiles without a migration path for existing settings.
3. Do not broaden cloud fallback behavior until fallback reasons and confidence thresholds are visible in diagnostics.
