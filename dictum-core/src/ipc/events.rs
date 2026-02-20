//! Event types emitted over the Tauri IPC channel.
//!
//! ## Channel names
//!
//! | Event | Channel |
//! |-------|---------|
//! | `TranscriptEvent` | `"dictum://transcript"` |
//! | `EngineStatusEvent` | `"dictum://status"` |
//! | `AudioActivityEvent` | `"dictum://activity"` |
//!
//! TypeScript mirrors live in `shared/ipc_types.ts`.
//! (ts-rs auto-generation is planned for P2-20.)

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Transcript events
// ---------------------------------------------------------------------------

/// Emitted on channel `"dictum://transcript"` when the pipeline produces output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEvent {
    /// Monotonically increasing event sequence number.
    pub seq: u64,
    /// One or more transcript segments from this inference pass.
    pub segments: Vec<TranscriptSegment>,
}

/// A single recognised speech segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    /// Unique ID for this utterance (stable across partial→final updates).
    pub id: String,
    /// Recognised text.
    pub text: String,
    /// Whether this is a streaming partial or a committed final.
    pub kind: SegmentKind,
    /// Model confidence in [0.0, 1.0], if available.
    pub confidence: Option<f32>,
}

/// Distinguishes streaming partials from committed finals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    /// Streaming partial — text may change on the next event with the same `id`.
    Partial,
    /// Committed final — the utterance is complete and will not change.
    Final,
}

// ---------------------------------------------------------------------------
// Audio activity events
// ---------------------------------------------------------------------------

/// Emitted on channel `"dictum://activity"` for each processed audio chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioActivityEvent {
    /// Monotonically increasing event sequence number.
    pub seq: u64,
    /// Root-mean-square level of the chunk in [0.0, 1.0].
    pub rms: f32,
    /// VAD decision for the current chunk.
    pub is_speech: bool,
}

// ---------------------------------------------------------------------------
// Engine status events
// ---------------------------------------------------------------------------

/// Emitted on channel `"dictum://status"` when the engine state changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatusEvent {
    pub status: EngineStatus,
    /// Optional human-readable detail (e.g. error message).
    pub detail: Option<String>,
}

/// Current state of the Dictum engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineStatus {
    /// Engine created but `start()` not yet called.
    Idle,
    /// Warming up model (loading weights, dummy inference).
    WarmingUp,
    /// Actively capturing audio and transcribing.
    Listening,
    /// Capture stopped; engine may be restarted.
    Stopped,
    /// Unrecoverable error — restart required.
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_event_serializes_with_camel_case_and_lowercase_kind() {
        let event = TranscriptEvent {
            seq: 7,
            segments: vec![TranscriptSegment {
                id: "utt-1".into(),
                text: "hello".into(),
                kind: SegmentKind::Partial,
                confidence: Some(0.91),
            }],
        };

        let json = serde_json::to_value(&event).expect("serialize transcript event");
        assert_eq!(json["seq"], 7);
        assert_eq!(json["segments"][0]["id"], "utt-1");
        assert_eq!(json["segments"][0]["text"], "hello");
        assert_eq!(json["segments"][0]["kind"], "partial");
        let conf = json["segments"][0]["confidence"]
            .as_f64()
            .expect("confidence should serialize as number");
        assert!((conf - 0.91).abs() < 1e-5);

        let round_trip: TranscriptEvent =
            serde_json::from_value(json).expect("deserialize transcript event");
        assert_eq!(round_trip.seq, 7);
        assert_eq!(round_trip.segments.len(), 1);
        assert_eq!(round_trip.segments[0].kind, SegmentKind::Partial);
    }

    #[test]
    fn engine_status_event_serializes_with_lowercase_status() {
        let event = EngineStatusEvent {
            status: EngineStatus::WarmingUp,
            detail: Some("loading model".into()),
        };

        let json = serde_json::to_value(&event).expect("serialize status event");
        assert_eq!(json["status"], "warmingup");
        assert_eq!(json["detail"], "loading model");

        let round_trip: EngineStatusEvent =
            serde_json::from_value(json).expect("deserialize status event");
        assert_eq!(round_trip.status, EngineStatus::WarmingUp);
        assert_eq!(round_trip.detail.as_deref(), Some("loading model"));
    }

    #[test]
    fn segment_kind_rejects_non_lowercase_values() {
        let invalid = r#""Partial""#;
        let err = serde_json::from_str::<SegmentKind>(invalid);
        assert!(err.is_err(), "expected invalid casing to fail");
    }

    #[test]
    fn audio_activity_event_serializes_with_camel_case_fields() {
        let event = AudioActivityEvent {
            seq: 3,
            rms: 0.18,
            is_speech: true,
        };

        let json = serde_json::to_value(&event).expect("serialize activity event");
        assert_eq!(json["seq"], 3);
        let rms = json["rms"]
            .as_f64()
            .expect("rms should serialize as number");
        assert!((rms - 0.18).abs() < 1e-5);
        assert_eq!(json["isSpeech"], true);

        let round_trip: AudioActivityEvent =
            serde_json::from_value(json).expect("deserialize activity event");
        assert_eq!(round_trip.seq, 3);
        assert!(round_trip.is_speech);
    }
}
