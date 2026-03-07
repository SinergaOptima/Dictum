# Dictum 0.1.8 Dev Plans 2-4

This document converts the current `0.1.8-dev.2` state into the next three execution steps.

Current `0.1.8-dev.2` baseline:

- guided 30-second tuning is integrated into onboarding and Settings
- dictation modes exist and affect transcript post-processing
- per-app profiles can override mode, phrase bias terms, and refinement
- active app/profile context is visible in Settings
- app profiles and learned corrections support import/export
- correction suggestions are mode-aware

The next three dev versions should now reduce implementation risk, deepen the profile/correction model, and prepare the branch for final release hardening.

## 0.1.8-dev.2 - UI Decomposition And Profile Usability

### Goal

Reduce regression risk by breaking the main Settings/live implementation into smaller modules while tightening the usability of the new context-aware features.

### Why This Comes Next

`dictum-ui/src/app/page.tsx` now carries onboarding, updates, tuning, history, diagnostics, profiles, and correction flows in one file. Adding more feature depth before reducing that blast radius will slow iteration and increase bug risk.

### Scope

- split the main page into focused components and hooks for:
  - onboarding + guided tune
  - settings core/runtime section
  - per-app profile management
  - learned correction management
  - updater controls
  - history/stats panels where practical
- keep the visual design stable
- preserve all current `0.1.8-dev.2` behavior
- improve app profile editing UX:
  - support editing an existing profile, not only add/delete
  - add starter presets for common apps such as:
    - `cursor.exe` -> `coding`
    - `code.exe` -> `coding`
    - `windowsterminal.exe` -> `command`
    - `slack.exe` -> `conversation`
- surface the currently active profile more clearly in Settings

### Out Of Scope

- backend engine replacement
- major visual redesign
- storage migrations for corrections

### Acceptance Criteria

- the main page is materially decomposed into smaller components or hooks with no behavior regression
- app profiles can be edited in-place
- at least four default app presets are available from the UI
- `npm run build`, `npm run typecheck`, and `cargo check` remain green

### Likely Files

- `dictum-ui/src/app/page.tsx`
- new components under `dictum-ui/src/components/`
- new hooks under `dictum-ui/src/hooks/`
- `dictum-ui/src/app/globals.css` only for minor supporting styles

### Risks

- moving code without changing behavior can still break subtle state interactions
- onboarding and update flows are the highest-risk areas during refactor

### Validation

- manual smoke of onboarding open/close/finish path
- manual smoke of profile create/edit/delete/import/export path
- manual smoke of copy diagnostics, updater controls, and correction import/export

## 0.1.8-dev.3 - Correction Intelligence And Profile-Aware Memory

### Goal

Make learned corrections feel materially smarter and more contextual, especially across prose, coding, and command workflows.

### Why This Comes After Dev.2

The UI needs to be easier to change before correction logic and visibility become deeper. Otherwise every correction improvement compounds the monolith problem.

### Scope

- add correction metadata needed for better management:
  - optional mode affinity
  - optional app-profile affinity
  - last-used timestamp
- improve correction ranking logic:
  - score by mode/app context
  - score by recency and hit count
  - avoid over-suggesting low-quality generic replacements
- add better correction visibility in UI:
  - show when a correction is global vs profile/mode-biased
  - show usage signal such as hits and last used
- allow saving a correction directly into the active app profile context
- add a “promote from suggestion” flow that captures active mode/profile metadata automatically

### Out Of Scope

- rewriting correction storage to a separate database table unless the current settings JSON approach blocks required metadata
- cloud-assisted correction services

### Acceptance Criteria

- correction suggestions differ meaningfully between at least two modes for the same low-confidence text
- users can see whether a correction is global or context-biased
- users can save a correction directly into the current active context
- correction import/export continues to work after metadata expansion
- backend and frontend checks remain green

### Likely Files

- `dictum-app/src/settings.rs`
- `dictum-app/src/main.rs`
- `dictum-ui/src/app/page.tsx`
- extracted correction-management components/hooks from `dev.2`
- `shared/ipc_types.ts`

### Risks

- settings-schema growth may become awkward if correction metadata expands too far
- scoring changes can make suggestions feel worse if not validated on realistic samples

### Validation

- add tests for correction ranking helpers where practical
- manual smoke using:
  - prose correction case
  - coding correction case
  - command correction case

## 0.1.8-dev.4 - Profile Depth, Diagnostics, And Release Prep

### Goal

Turn the new context-aware feature set into a release-candidate path by strengthening diagnostics, benchmarkability, and migration confidence.

### Why This Comes Before Dev.5

`dev.5` should be reserved for stabilization, not new architecture. `dev.4` is the point where the feature set becomes release-shaped and observable.

### Scope

- add richer diagnostics for active context:
  - active foreground app
  - matched app profile
  - effective mode
  - effective phrase bias term count
  - effective refinement state
- include that context in the diagnostics bundle export
- add benchmark notes or fixture docs for:
  - prose mode
  - coding mode
  - command mode
- review settings migration behavior for:
  - new app-profile fields
  - correction metadata added in `dev.3`
- tighten profile import/export expectations:
  - validate bad JSON more clearly
  - reject malformed profile entries with readable errors
- prepare a focused `dev.5` stabilization checklist

### Out Of Scope

- public release tagging
- installer publication
- backend replacement

### Acceptance Criteria

- diagnostics export includes active context information without leaking secrets
- malformed profile imports fail with readable user-facing errors
- benchmark docs cover all three dictation modes
- migration behavior is documented well enough for `dev.5` stabilization
- the branch is feature-complete enough that `dev.5` can focus mainly on bug fixing and release gating

### Likely Files

- `dictum-app/src/commands.rs`
- `dictum-app/src/state.rs`
- `dictum-ui/src/app/page.tsx`
- `shared/ipc_types.ts`
- `benchmarks/README.md`
- release or roadmap docs under `docs/`

### Risks

- diagnostics additions can become noisy if not kept concise
- import validation must not silently discard user data

### Validation

- `cargo check`
- `cargo test -p dictum-app`
- `npm run build`
- `npm run typecheck`
- manual export/import and diagnostics smoke tests

### Dev.5 Stabilization Checklist

- run app-profile import/export smoke tests with:
  - valid profile array
  - empty array
  - invalid `dictationMode`
  - missing `appMatch`
- export a diagnostics file from the Stats tab and verify it contains:
  - active foreground app
  - matched profile name/id
  - effective mode
  - correction diagnostics
- rerun the smoke benchmark baseline and compare against `benchmarks/baselines/smoke-baseline.json`
- verify guided tune still works from both onboarding and Settings
- verify suggestion promotion works for:
  - global correction save
  - mode-only correction save
  - profile-scoped correction save

## Suggested Sequencing

1. `0.1.8-dev.2`
   - refactor first
   - editable app profiles
   - app presets
2. `0.1.8-dev.3`
   - correction metadata and smarter ranking
   - active-context correction save flow
3. `0.1.8-dev.4`
   - diagnostics and benchmark coverage
   - migration and import-hardening pass

## Release Readiness Note

If `0.1.8-dev.2` or `0.1.8-dev.3` reveals that the current settings JSON structure is becoming operationally brittle, that should be treated as a design warning and resolved before `0.1.8-dev.5` rather than deferred into the public release.
