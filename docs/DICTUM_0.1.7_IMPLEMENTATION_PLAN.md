# Dictum 0.1.7 Implementation Plan

This plan turns the March 2026 audit into an execution-ready build plan for `v0.1.7`.

## Release Intent

`v0.1.7` is a reliability and instrumentation release.

It should make Dictum:

- easier to diagnose
- safer to update
- more scalable for history/stat usage
- less likely to drift in runtime configuration behavior

It should not become a broad feature release. Per-app profiles, dictation modes, and larger correction-memory improvements remain `v0.1.8` work unless they are required as enabling refactors.

## Scope Lock

### In Scope

- deeper pipeline and runtime diagnostics
- diagnostics bundle export
- history and stats performance work
- runtime settings/env consolidation
- updater and release hardening
- benchmark fixture pack and baseline reporting
- small supporting UX improvements that expose reliability data without redesigning the UI

### Out Of Scope

- pill redesign
- major visual redesign
- backend replacement work such as Faster-Whisper or whisper.cpp
- per-app profiles
- dictation modes
- settings sync

## Success Criteria

`v0.1.7` is done when all of the following are true:

- perf diagnostics expose measured timings for capture-adjacent work, VAD, inference, transform, finalize, inject, and persist
- users can export a local diagnostics bundle without external services
- history search and stats remain responsive at materially larger history sizes
- runtime settings application uses one consistent code path across startup, tuning, and manual updates
- updater behavior is smoke-tested from the prior public installer
- benchmark fixtures and baseline reports exist in-repo and are runnable by another engineer

## Workstreams

## 1. Diagnostics And Observability

### Goal

Expose enough structured runtime evidence to diagnose latency, fallback, and reliability issues without guessing from logs.

### Deliverables

- extend `PerfSnapshot` with:
  - resample/capture-adjacent timing
  - VAD timing
  - inference timing
  - duplicate suppression count
  - partial rescue count
  - fallback attempt/success counts by type
- add diagnostics bundle command and shared type
- add a UI entry point to export diagnostics
- add a compact reliability summary in settings or stats

### Likely Files

- `dictum-app/src/state.rs`
- `dictum-app/src/main.rs`
- `dictum-app/src/commands.rs`
- `shared/ipc_types.ts`
- `dictum-ui/src/lib/tauri.ts`
- `dictum-ui/src/app/page.tsx`
- possibly a new diagnostics module in `dictum-app/src/`

### Implementation Notes

- prefer additive IPC changes
- keep the exported bundle scrubbed of secrets by default
- include safe runtime settings, app version, update repo slug, model profile, ORT settings, device list, and history volume metrics

### Validation

- unit tests for any new diagnostics aggregation logic
- manual verification that bundle export works on a normal install
- confirm bundle contains no API key or raw secret material

## 2. History And Stats Performance

### Goal

Remove the current scale limit caused by decrypting and filtering large sets of history rows in-process.

### Deliverables

- redesign `get_history` so paging happens before heavy application-layer work where possible
- redesign `get_stats` so aggregation is database-driven or uses pre-aggregated/index-friendly metadata
- add schema support for searchable metadata or an FTS/sidecar strategy
- document migration/backfill behavior

### Likely Files

- `dictum-app/src/storage.rs`
- any migration helper added under `dictum-app/src/`
- `shared/ipc_types.ts` only if the payload shape changes
- `dictum-ui/src/app/page.tsx` if loading behavior or empty-state messaging changes

### Implementation Notes

- do not weaken privacy guarantees casually; if plaintext search metadata is introduced, document the tradeoff explicitly
- prefer additive schema migration with one-time backfill
- keep the public UI behavior mostly the same

### Validation

- add storage tests for history paging, query correctness, and stats accuracy
- seed a large local dataset and verify that history search and stats remain responsive
- verify retention prune still works after schema changes

## 3. Runtime Config Consolidation

### Goal

Make settings behavior consistent across startup, manual settings updates, and auto-tune flows.

### Deliverables

- central helper for applying runtime env/config changes
- remove duplicated env write logic from command handlers where possible
- ensure all runtime-affecting settings follow one normalization and apply path
- document which settings apply immediately and which apply next session if any remain

### Likely Files

- `dictum-app/src/settings.rs`
- `dictum-app/src/commands.rs`
- `dictum-app/src/main.rs`

### Implementation Notes

- keep behavior stable for existing settings keys
- avoid changing user-facing settings semantics in `0.1.7`

### Validation

- update or add tests around settings normalization and runtime application
- verify that tuning and manual setting changes produce the same persisted/runtime state

## 4. Updater And Release Hardening

### Goal

Turn the updater and release process into a repeatable, supportable shipping path.

### Deliverables

- codify release smoke-test checklist
- verify updater negative paths:
  - invalid repo slug
  - missing checksum asset
  - checksum mismatch
  - invalid signature
- ensure default repo slug remains canonical in frontend and backend
- add release notes/checklist doc updates if needed

### Likely Files

- `dictum-app/src/commands.rs`
- `dictum-ui/src/app/page.tsx`
- `.github/workflows/release-windows.yml`
- `docs/GITHUB_RELEASE.md`

### Implementation Notes

- avoid another release-path rewrite unless the current flow proves insufficient
- prefer testable command-level behavior and documented release procedures

### Validation

- smoke-test update check and install from the current public installer
- verify release artifact set and signature/checksum expectations
- run a canary or dry-run release path before tagging `v0.1.7`

## 5. Benchmark Fixtures And Baselines

### Goal

Stop making tuning decisions from anecdotal local testing.

### Deliverables

- committed fixture pack layout for:
  - quiet speech
  - whisper speech
  - noisy room
  - long form
- documentation for fixture naming and expected transcript files
- baseline benchmark reports for at least one reference hardware setup
- script or instructions to rerun benchmarks consistently

### Likely Files

- fixture assets under a committed benchmark directory
- `dictum-core/src/bin/benchmark.rs` if needed for better output or metadata
- docs describing how to run and compare reports

### Implementation Notes

- keep fixtures reasonably small so the repo remains usable
- if full audio cannot live in the repo, define a reproducible fixture acquisition strategy and include at least a minimal committed smoke set

### Validation

- run benchmark command against committed fixtures
- verify report output is stable enough for before/after comparisons

## 6. Minimal UX Surface Improvements

### Goal

Expose reliability improvements without redesigning the product.

### Deliverables

- export diagnostics action in the UI
- compact reliability summary block
- clearer history empty/loading states if backend behavior changes
- clearer update state/error wording where release hardening exposes better information

### Likely Files

- `dictum-ui/src/app/page.tsx`
- supporting hooks/components if extracted

### Implementation Notes

- keep visual design unchanged
- do not let this workstream grow into a page redesign

### Validation

- keyboard-only check for diagnostics export and update/error visibility
- verify no regression to current onboarding/update/settings flows

## Execution Sequence

### Phase 1: Foundation

1. Implement runtime config consolidation.
2. Add diagnostics data structures and instrumentation plumbing.
3. Define shared IPC types for richer perf and diagnostics export.

### Phase 2: Data And Reliability

1. Rework history/stats storage paths and migration.
2. Implement diagnostics bundle export.
3. Improve update-path validation and release checklist support.

### Phase 3: Product Surface

1. Expose diagnostics and reliability summary in UI.
2. Improve update and history messaging where required.
3. Add benchmark fixtures, docs, and baseline reports.

### Phase 4: Release Readiness

1. Run full validation pass.
2. Smoke-test updater from current public installer.
3. Build installer.
4. Tag and publish `v0.1.7`.

## Task Breakdown

## A. Diagnostics

- add stage timing collectors for inference, VAD, and capture-adjacent work
- add counters for duplicate suppression and fallback/rescue outcomes
- expand shared `PerfSnapshot`
- define diagnostics bundle payload
- implement export command and UI action

## B. Storage

- choose history search strategy
- add migration/backfill path
- refactor `get_history`
- refactor `get_stats`
- test large-dataset behavior

## C. Settings Runtime

- centralize apply-runtime logic
- update startup path to use the same helper
- update auto-tune and benchmark auto-tune to use the same helper
- verify normalization remains stable

## D. Release Hardening

- add or update release checklist doc
- verify command-level update errors are user-readable
- test checksum/signature/repo failures
- confirm canonical repo slug in both layers

## E. Benchmarks

- commit fixture layout
- add expected transcripts
- run baseline benchmark
- publish baseline report in docs or committed JSON

## Risks And Mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| History search redesign weakens privacy guarantees | High | choose metadata strategy explicitly, document tradeoffs, avoid accidental plaintext leakage |
| Diagnostics bundle leaks secrets | High | use an allowlist payload, never serialize API keys or raw env vars wholesale |
| Richer diagnostics create UI churn | Medium | keep UI changes minimal and reuse existing settings/stats surfaces |
| Release hardening work delays shipping | Medium | scope it to smoke tests, checklist, and command validation, not a full release pipeline rewrite |
| Fixture pack becomes too large for the repo | Medium | commit a minimal smoke set and document an extended local pack if needed |

## Recommended Branching And Delivery

- work on a feature branch prefixed with `codex/`
- land by workstream in reviewable PR-sized commits
- keep schema changes and diagnostics bundle changes isolated enough to test independently

Suggested delivery order:

1. runtime config consolidation
2. diagnostics plumbing
3. storage changes
4. UI exposure
5. release hardening and fixture pack

## Final Release Gate

Do not cut `v0.1.7` until:

- `cargo check` passes
- `cargo test -p dictum-core` passes
- relevant app/storage tests pass
- `npm run typecheck` passes
- `npm run build` passes
- updater smoke test passes from the previous public installer
- installer build succeeds
- release artifact checklist is satisfied

## Immediate Next Step

Start with Workstream 3 and Workstream 1 together:

1. consolidate runtime settings application
2. add richer diagnostics structures and timing collection

That gives the rest of `0.1.7` a stable foundation and makes subsequent storage and updater work easier to verify.
