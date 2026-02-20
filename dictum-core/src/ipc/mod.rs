//! IPC types serialised over the Tauri event bus.
//!
//! All types derive `serde::Serialize` + `serde::Deserialize` so they can be
//! emitted via `app.emit_all(...)` and mirrored in `shared/ipc_types.ts`.

pub mod events;
