# Dictum

Dictum is a desktop dictation app focused on fast, local, low-latency speech-to-text.

It combines:
- A Rust transcription engine (`dictum-core`)
- A Tauri desktop host (`dictum-app`)
- A Next.js settings/transcript UI (`dictum-ui`)
- A compact floating status pill (`/pill`)

## Core capabilities

- Local-first transcription pipeline
- Live dictation with inline text injection
- Global shortcut control
- Audio sensitivity controls and microphone calibration
- Snippets and custom dictionary replacements
- Optional OpenAI cloud fallback for difficult utterances

## Project structure

- `dictum-core`: audio pipeline, VAD, inference, transcript events
- `dictum-app`: Tauri runtime, commands, persistence, shortcuts, tray integration
- `dictum-ui`: desktop UI and pill UI (Next.js)
- `shared`: IPC TypeScript types shared by UI and app

## Roadmap

### Near term
- Improve low-volume recognition reliability and confidence handling.
- Expand model profile support and tuning for high-end GPUs.
- Continue UI refinement in settings and transcript quality feedback.
- Harden release quality (signed artifacts, checksum visibility, rollback discipline).

### Mid term
- Per-app dictation profiles (different behavior for IDE/chat/docs).
- Advanced correction pipeline with phrase-level reprocessing on uncertain segments.
- Better live quality indicators and optional post-utterance rewrite controls.
- Richer snippet/dictionary management UX (search, import/export, team presets).

### Longer term
- Pluggable inference backends and optional quantized model paths.
- Multi-device profile sync with privacy-first defaults.
- Opt-in telemetry bundle export for performance and recognition diagnostics.
- Enterprise policy controls for cloud fallback, retention, and governance.

## Release and updates

- Release flow is stable-only (no beta/nightly channels).
- Windows release artifacts are published via GitHub Releases by the Windows release workflow.
- In-app updater checks the latest stable GitHub release and can launch installer updates from Settings.

### Windows code signing (required for trusted publisher)

To avoid `Unknown publisher` in Windows UAC, release binaries must be signed with a publicly trusted code-signing certificate whose subject includes `Lattice Labs`.

Required GitHub repository secrets:

- `WINDOWS_CERT_BASE64`: Base64-encoded `.pfx` file.
- `WINDOWS_CERT_PASSWORD`: Password for the `.pfx`.
- `WINDOWS_CERT_EXPECTED_SUBJECT`: Set to `Lattice Labs` (or your exact legal signer subject substring).

PowerShell helper to prepare `WINDOWS_CERT_BASE64` locally:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("C:\path\to\lattice-labs-codesign.pfx"))
```

Workflow enforcement now fails release builds when:

- signing secrets are missing,
- Authenticode status is not `Valid`,
- or signer subject does not match the expected `Lattice Labs` value.

### Updater security model

- Installer URL must be HTTPS.
- Update metadata consumes `SHA256SUMS.txt` from release assets.
- Installer bytes are hashed in-app and must match the expected SHA-256 before launch.
- On Windows, Authenticode signature validation must return `Valid` before installer launch.

### Update UX behavior

- Manual `Check for updates` and `Install available update` actions in Settings.
- Optional startup auto-check (non-blocking).
- Optional auto-install when idle (not listening, idle grace window).
- `Remind later` and `Skip version` controls.

### Updater telemetry

- Update events (check/install/defer/fail) are stored locally in the app UI state/local storage.
- Settings includes a recent updater event feed.
- `Copy Update Log` exports updater telemetry JSON to clipboard for diagnostics.

## Acknowledgment

This project was built with AI-assisted development.
