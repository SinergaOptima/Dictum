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

## Acknowledgment

This project was built with AI-assisted development.
