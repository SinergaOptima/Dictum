/**
 * shared/ipc_types.ts
 *
 * TypeScript mirror of Rust IPC types defined in:
 *   dictum-core/src/ipc/events.rs
 *   dictum-core/src/audio/device.rs
 *
 * ⚠ Manual sync required for now.
 * Automatic code generation via ts-rs is planned for P2-20.
 *
 * All field names use camelCase to match the `#[serde(rename_all = "camelCase")]`
 * attribute on the Rust structs.
 */

// ---------------------------------------------------------------------------
// Transcript events  (channel: "dictum://transcript")
// ---------------------------------------------------------------------------

/**
 * Emitted by the engine when inference produces output.
 *
 * Rust: `TranscriptEvent`
 */
export interface TranscriptEvent {
  /** Monotonically increasing event sequence number. */
  seq: number;
  /** One or more transcript segments from this inference pass. */
  segments: TranscriptSegment[];
}

/**
 * A single recognised speech segment.
 *
 * Rust: `TranscriptSegment`
 */
export interface TranscriptSegment {
  /** Unique ID for this utterance — stable across partial → final updates. */
  id: string;
  /** Recognised text. */
  text: string;
  /** Whether this is a streaming partial or a committed final result. */
  kind: SegmentKind;
  /** Model confidence in [0, 1], or null if not available. */
  confidence: number | null;
}

/**
 * Rust: `SegmentKind`
 *
 * Serialised as lowercase string by `#[serde(rename_all = "lowercase")]`.
 */
export type SegmentKind = "partial" | "final";

// ---------------------------------------------------------------------------
// Engine status events  (channel: "dictum://status")
// ---------------------------------------------------------------------------

/**
 * Emitted when the engine's state changes.
 *
 * Rust: `EngineStatusEvent`
 */
export interface EngineStatusEvent {
  status: EngineStatus;
  /** Optional human-readable detail (e.g. error message). */
  detail: string | null;
}

/**
 * Current state of the Dictum engine.
 *
 * Rust: `EngineStatus`
 *
 * Serialised as lowercase string by `#[serde(rename_all = "lowercase")]`.
 */
export type EngineStatus =
  | "idle"
  | "warmingup"
  | "listening"
  | "stopped"
  | "error";

// ---------------------------------------------------------------------------
// Audio activity events (channel: "dictum://activity")
// ---------------------------------------------------------------------------

/**
 * Emitted on every processed chunk to drive live speaking indicators.
 *
 * Rust: `AudioActivityEvent`
 */
export interface AudioActivityEvent {
  /** Monotonically increasing event sequence number. */
  seq: number;
  /** Root-mean-square level of the chunk in [0, 1]. */
  rms: number;
  /** True if VAD classified current chunk as speech. */
  isSpeech: boolean;
}

// ---------------------------------------------------------------------------
// Audio device info  (returned by list_audio_devices command)
// ---------------------------------------------------------------------------

/**
 * Metadata about an audio input device.
 *
 * Rust: `DeviceInfo`
 */
export interface DeviceInfo {
  /** Human-readable device name reported by the OS. */
  name: string;
  /** Whether this is the system default input device. */
  isDefault: boolean;
}

// ---------------------------------------------------------------------------
// Runtime settings (returned by get_runtime_settings/set_runtime_settings)
// ---------------------------------------------------------------------------

export interface RuntimeSettings {
  /** Whisper model profile name, e.g. "small", "small.en", "large-v3-turbo". */
  modelProfile: string;
  /** Runtime performance profile preset. */
  performanceProfile:
    | "whisper_balanced_english"
    | "stability_long_form"
    | "balanced_general"
    | "latency_short_utterance";
  /** Global push-to-talk toggle shortcut. */
  toggleShortcut: string;
  /** ONNX execution provider preference: "auto" | "cpu" | "directml". */
  ortEp: string;
  /** Decode hint: "auto" | "english" | "mandarin" | "russian". */
  languageHint: string;
  /** Pill visualizer sensitivity multiplier in [1, 20]. */
  pillVisualizerSensitivity: number;
  /** Activity meter sensitivity multiplier in [1, 20]. */
  activitySensitivity: number;
  /** Activity noise gate in RMS units. */
  activityNoiseGate: number;
  /** Approximate clipping threshold in RMS units. */
  activityClipThreshold: number;
  /** Input adaptive gain multiplier in [0.5, 8]. */
  inputGainBoost: number;
  /** Enables an additional quality-focused decode pass for final utterances. */
  postUtteranceRefine: boolean;
  /** Phrase bias terms for domain vocabulary boosting. */
  phraseBiasTerms: string[];
  /** Whether an OpenAI API key is stored locally. */
  hasOpenAiApiKey: boolean;
  /** Whether cloud fallback is enabled. */
  cloudOptIn: boolean;
  /** Whether transcript history is persisted. */
  historyEnabled: boolean;
  /** Retention horizon in days. */
  retentionDays: number;
}

// ---------------------------------------------------------------------------
// History / stats / dictionary / snippets
// ---------------------------------------------------------------------------

export interface HistoryItem {
  id: string;
  createdAt: string;
  text: string;
  source: string;
  latencyMs: number;
  wordCount: number;
  charCount: number;
  dictionaryApplied: boolean;
  snippetApplied: boolean;
}

export interface HistoryPage {
  items: HistoryItem[];
  total: number;
  page: number;
  pageSize: number;
}

export interface StatsBucket {
  date: string;
  utterances: number;
  words: number;
  chars: number;
  avgLatencyMs: number;
}

export interface StatsPayload {
  rangeDays: number;
  totalUtterances: number;
  totalWords: number;
  totalChars: number;
  avgLatencyMs: number;
  buckets: StatsBucket[];
}

export interface DictionaryEntry {
  id: string;
  term: string;
  aliases: string[];
  language: string | null;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface SnippetEntry {
  id: string;
  trigger: string;
  expansion: string;
  mode: "slash" | "phrase";
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface PrivacySettings {
  historyEnabled: boolean;
  retentionDays: number;
  cloudOptIn: boolean;
}

export interface PerfStageSnapshot {
  count: number;
  meanMs: number;
  p50Ms: number;
  p95Ms: number;
  p99Ms: number;
  maxMs: number;
}

export interface AppDiagnostics {
  injectCalls: number;
  injectSuccess: number;
  finalSegmentsSeen: number;
  fallbackStubTyped: number;
  pipelineFramesIn: number;
  pipelineFramesResampled: number;
  pipelineVadWindows: number;
  pipelineVadSpeech: number;
  pipelineInferenceCalls: number;
  pipelineInferenceErrors: number;
  pipelineSegmentsEmitted: number;
  pipelineFallbackEmitted: number;
}

export interface PerfSnapshot {
  diagnostics: AppDiagnostics;
  transformMs: PerfStageSnapshot;
  injectMs: PerfStageSnapshot;
  persistMs: PerfStageSnapshot;
  finalizeMs: PerfStageSnapshot;
}
