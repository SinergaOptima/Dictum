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

## Development

Requirements:
- Rust stable toolchain
- Node.js 20+
- npm

Run locally:

```powershell
cd dictum-app
cargo tauri dev
```

## Windows packaging

Build release exe + NSIS installer:

```powershell
cd dictum-app
cargo tauri build --bundles nsis
```

Output artifacts:
- `target/release/dictum.exe`
- `target/release/bundle/nsis/*-setup.exe`

## Cloud dictation key

You can set the OpenAI API key directly in **Settings > Privacy**.
Dictum stores whether a key exists and uses it for cloud fallback when enabled.

## CI and release

- CI: `.github/workflows/ci.yml`
- Windows release workflow: `.github/workflows/release-windows.yml`

For GitHub setup, pushing, and Windows code-signing secrets, see:
- `docs/GITHUB_RELEASE.md`

## Acknowledgment

This project was built with AI-assisted development.
