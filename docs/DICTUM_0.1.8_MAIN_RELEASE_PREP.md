# Dictum 0.1.8 Main Release Prep

This document turns the current `0.1.8-dev.3` branch state into the remaining execution path for the public `0.1.8` release.

## Current Baseline

`0.1.8-dev.3` now includes:

- guided 30-second onboarding tune
- dictation modes
- per-app profiles with normalized executable matching
- per-app phrase bias and refinement overrides
- active app/profile visibility
- profile import/export
- learned correction import/export, filtering, and scoped editing
- context-aware suggestion promotion
- correction health diagnostics
- prune actions for unused, orphaned, and stale correction rules
- richer diagnostics export and stats visibility

## Remaining Dev Cuts

### `0.1.8-dev.4`

Goal:
- finish release-shaped diagnostics and benchmark coverage

Required work:
- expand diagnostics export review around active context and correction health
- add benchmark notes and expectations for `conversation`, `coding`, and `command`
- verify settings migration behavior for correction metadata and app-profile fields
- tighten import/export edge cases one more time based on `dev.3` usage

Acceptance:
- diagnostics export is support-ready
- benchmark docs cover all dictation modes
- no migration surprises remain for existing settings files

### `0.1.8-dev.5`

Goal:
- stabilization only

Required work:
- smoke-test onboarding, guided tune, profile import/export, correction promotion, diagnostics export, updater settings, tray behavior, and single-instance handling
- resolve any regressions found from `dev.4`
- confirm release workflow behavior is acceptable for the public cut
- freeze scope

Acceptance:
- no known blocking regressions remain
- release checklist is decision-complete for public `0.1.8`

## Public `0.1.8` Release Gates

Before tagging `v0.1.8`, verify all of the following:

- `cargo check`
- `cargo test -p dictum-app`
- `cargo test -p dictum-core`
- `npm run build`
- `npm run typecheck`
- `node ./scripts/smoke-ui.mjs`
- packaged installer builds successfully
- updater path works from the previous public installer
- diagnostics export writes a file and includes active context plus correction diagnostics
- no orphaned correction rules remain in the default test profile set
- release workflow signing behavior is explicitly accepted for the chosen release path

## Known Watch Items

- The GitHub Windows signing path has bounded retries/timeouts now, but it still needs one clean real-world release run before being treated as fully trusted again.
- `dictum-ui/tsconfig.tsbuildinfo` remains generated noise and should stay out of commits.
- The existing `Newsreader` font override warning still appears during Next production builds.

## Recommended Next Actions

1. Build and package `0.1.8-dev.3`.
2. Use `dev.4` for diagnostics, benchmark, and migration hardening only.
3. Use `dev.5` for stabilization and release gating only.
4. Tag public `v0.1.8` only after `dev.5` passes the release checklist end to end.
