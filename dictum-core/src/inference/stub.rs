//! `StubModel` — placeholder backend that echoes metadata without real inference.
//!
//! Used during development before ONNX Whisper is integrated (Phase 1).
//! Produces a deterministic partial + final transcript so the full UI/IPC
//! pipeline can be exercised end-to-end.

use crate::buffering::chunk::AudioChunk;
use crate::error::Result;
use crate::inference::SpeechModel;
use crate::ipc::events::{SegmentKind, TranscriptSegment};
use tracing::debug;

/// Echo-style stub model.
///
/// For every chunk of non-trivial length it emits:
/// 1. A partial segment: `"…"` (simulates streaming latency)
/// 2. A final segment: `"[stub: <N> samples @ <SR> Hz]"`
pub struct StubModel {
    utterance_count: u32,
}

impl StubModel {
    pub fn new() -> Self {
        Self { utterance_count: 0 }
    }
}

impl Default for StubModel {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechModel for StubModel {
    fn warm_up(&mut self) -> Result<()> {
        debug!("StubModel::warm_up — no-op");
        Ok(())
    }

    fn transcribe(&mut self, chunk: &AudioChunk, partial: bool) -> Result<Vec<TranscriptSegment>> {
        if chunk.samples.len() < 160 {
            return Ok(vec![]);
        }

        self.utterance_count += 1;
        let id = format!("stub-{}", self.utterance_count);

        let segments = if partial {
            vec![TranscriptSegment {
                id: id.clone(),
                text: "\u{2026}".to_string(), // "…"
                kind: SegmentKind::Partial,
                confidence: None,
            }]
        } else {
            vec![TranscriptSegment {
                id,
                text: format!(
                    "[stub: {} samples @ {} Hz]",
                    chunk.samples.len(),
                    chunk.sample_rate
                ),
                kind: SegmentKind::Final,
                confidence: Some(1.0),
            }]
        };

        Ok(segments)
    }

    fn reset(&mut self) {
        debug!("StubModel::reset");
    }
}
