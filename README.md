# Dictum

## Product Description

Dictum is a desktop dictation app built for fast, local, low-latency speech-to-text on Windows.  
It focuses on high-quality voice capture, quick text insertion, and practical controls for daily writing and workflow automation.

## Tech Stack

- Rust (`dictum-core`) for audio pipeline, VAD, inference, and transcription engine logic
- Tauri (`dictum-app`) for desktop runtime, windowing, tray integration, and native commands
- Next.js + React (`dictum-ui`) for the main app UI and floating pill interface
- TypeScript shared IPC contracts (`shared`) for frontend/backend command and event typing

## Roadmap

- Improve low-volume recognition reliability and confidence handling
- Continue model/runtime tuning for high-end GPU systems
- Expand correction memory and live rewrite quality
- Improve updater resilience, rollback safety, and release hardening
- Add richer snippet/dictionary management and per-context workflow controls

## Acknowledgement

This project was built with AI-assisted development.
