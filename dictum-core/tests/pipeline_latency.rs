use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

use dictum_core::buffering::{chunk::AudioChunk, create_audio_ring, Producer};
use dictum_core::engine::{pipeline, EngineConfig};
use dictum_core::ipc::events::{EngineStatus, SegmentKind, TranscriptEvent, TranscriptSegment};
use dictum_core::vad::{VadDecision, VoiceActivityDetector};
use dictum_core::{DictumError, ModelHandle, SpeechModel};
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::TryRecvError;

struct AlwaysSpeechVad;

impl VoiceActivityDetector for AlwaysSpeechVad {
    fn classify(&mut self, _chunk: &AudioChunk) -> VadDecision {
        VadDecision::Speech
    }

    fn reset(&mut self) {}
}

struct DelayModel {
    delay: Duration,
}

impl DelayModel {
    fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

impl SpeechModel for DelayModel {
    fn warm_up(&mut self) -> std::result::Result<(), DictumError> {
        Ok(())
    }

    fn transcribe(
        &mut self,
        _chunk: &AudioChunk,
        partial: bool,
    ) -> std::result::Result<Vec<TranscriptSegment>, DictumError> {
        thread::sleep(self.delay);

        Ok(vec![TranscriptSegment {
            id: "latency-test".into(),
            text: "ok".into(),
            kind: if partial {
                SegmentKind::Partial
            } else {
                SegmentKind::Final
            },
            confidence: None,
        }])
    }

    fn reset(&mut self) {}
}

fn recv_event_with_timeout(
    rx: &mut broadcast::Receiver<TranscriptEvent>,
    timeout: Duration,
) -> TranscriptEvent {
    let start = Instant::now();
    loop {
        match rx.try_recv() {
            Ok(ev) => return ev,
            Err(TryRecvError::Empty) => {
                if start.elapsed() >= timeout {
                    panic!("timed out waiting for transcript event");
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(TryRecvError::Lagged(_)) => continue,
            Err(TryRecvError::Closed) => panic!("transcript channel closed unexpectedly"),
        }
    }
}

#[test]
fn first_transcript_latency_under_500ms() {
    let (mut producer, consumer) = create_audio_ring();
    producer.push_slice(&vec![0.2; 960]);

    let running = Arc::new(AtomicBool::new(true));
    let seq = Arc::new(AtomicU64::new(0));
    let (transcript_tx, mut transcript_rx) = broadcast::channel(16);
    let (status_tx, _) = broadcast::channel(8);
    let (activity_tx, _) = broadcast::channel(8);

    let mut config = EngineConfig::default();
    config.target_sample_rate = 16_000;
    config.min_speech_samples = 960;
    config.max_speech_samples = 16_000;

    let ctx = pipeline::PipelineContext {
        config,
        model: ModelHandle::new(DelayModel::new(Duration::from_millis(20))),
        vad: Box::new(AlwaysSpeechVad),
        consumer,
        running: Arc::clone(&running),
        transcript_tx,
        status_tx,
        activity_tx,
        status: Arc::new(Mutex::new(EngineStatus::Idle)),
        seq,
        capture_sample_rate: 16_000,
        diagnostics: Arc::new(pipeline::PipelineDiagnostics::default()),
    };

    let start = Instant::now();
    let handle = thread::spawn(move || pipeline::run(ctx));

    let first = recv_event_with_timeout(&mut transcript_rx, Duration::from_secs(2));
    let elapsed = start.elapsed();

    running.store(false, Ordering::SeqCst);
    handle.join().expect("pipeline thread panicked");

    assert_eq!(first.segments[0].kind, SegmentKind::Partial);
    assert!(
        elapsed < Duration::from_millis(500),
        "TTFW too high: {:?} (target < 500ms)",
        elapsed
    );
}
