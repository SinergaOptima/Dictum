/**
 * Typed wrappers around @tauri-apps/api primitives.
 *
 * Centralises all Tauri IPC calls so command names and payload types are
 * defined in one place. TypeScript will catch mismatches at compile time.
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
import { getVersion as tauriGetVersion } from "@tauri-apps/api/app";
import type {
  AudioActivityEvent,
  DictionaryEntry,
  EngineStatus,
  TranscriptEvent,
  EngineStatusEvent,
  DeviceInfo,
  HistoryPage,
  PrivacySettings,
  RuntimeSettings,
  SnippetEntry,
  StatsPayload,
  PerfSnapshot,
  ModelProfileMetadata,
  ModelProfileRecommendation,
  AutoTuneResult,
  BenchmarkAutoTuneResult,
  AppUpdateInfo,
  LearnedCorrection,
} from "@shared/ipc_types";

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

export const startEngine = (deviceName?: string | null): Promise<void> =>
  tauriInvoke("start_engine", {
    deviceName: deviceName ?? null,
  });

export const stopEngine = (): Promise<void> =>
  tauriInvoke("stop_engine");

export const getStatus = (): Promise<EngineStatus> =>
  tauriInvoke("get_status");

export const getAppVersion = (): Promise<string> =>
  tauriGetVersion();

export const listAudioDevices = (): Promise<DeviceInfo[]> =>
  tauriInvoke("list_audio_devices");

export const setPreferredInputDevice = (
  deviceName: string | null
): Promise<void> =>
  tauriInvoke("set_preferred_input_device", {
    deviceName,
  });

export const getPreferredInputDevice = (): Promise<string | null> =>
  tauriInvoke("get_preferred_input_device");

export const getRuntimeSettings = (): Promise<RuntimeSettings> =>
  tauriInvoke("get_runtime_settings");

export const getModelProfileCatalog = (): Promise<ModelProfileMetadata[]> =>
  tauriInvoke("get_model_profile_catalog");

export const getModelProfileRecommendation = (): Promise<ModelProfileRecommendation> =>
  tauriInvoke("get_model_profile_recommendation");

export const runAutoTune = (): Promise<AutoTuneResult> =>
  tauriInvoke("run_auto_tune");

export const runBenchmarkAutoTune = (
  ambientP90: number,
  whisperP70: number,
  normalP80: number,
  finalizeP95Ms: number,
  fallbackRatePct: number,
): Promise<BenchmarkAutoTuneResult> =>
  tauriInvoke("run_benchmark_auto_tune", {
    ambientP90,
    whisperP70,
    normalP80,
    finalizeP95Ms,
    fallbackRatePct,
  });

export const checkForAppUpdate = (
  repoSlug?: string | null,
): Promise<AppUpdateInfo> =>
  tauriInvoke("check_for_app_update", {
    repoSlug: repoSlug ?? null,
  });

export const downloadAndInstallAppUpdate = (
  downloadUrl: string,
  assetName?: string | null,
  silentInstall?: boolean | null,
  autoExit?: boolean | null,
  expectedSha256?: string | null,
): Promise<string> =>
  tauriInvoke("download_and_install_app_update", {
    downloadUrl,
    assetName: assetName ?? null,
    silentInstall: silentInstall ?? null,
    autoExit: autoExit ?? null,
    expectedSha256: expectedSha256 ?? null,
  });

export const setRuntimeSettings = (
  modelProfile?: string | null,
  performanceProfile?: string | null,
  toggleShortcut?: string | null,
  ortEp?: string | null,
  ortIntraThreads?: number | null,
  ortInterThreads?: number | null,
  ortParallel?: boolean | null,
  languageHint?: string | null,
  pillVisualizerSensitivity?: number | null,
  activitySensitivity?: number | null,
  activityNoiseGate?: number | null,
  activityClipThreshold?: number | null,
  inputGainBoost?: number | null,
  postUtteranceRefine?: boolean | null,
  phraseBiasTerms?: string[] | null,
  openAiApiKey?: string | null,
  cloudMode?: string | null,
  cloudOptIn?: boolean | null,
  reliabilityMode?: boolean | null,
  onboardingCompleted?: boolean | null,
  historyEnabled?: boolean | null,
  retentionDays?: number | null,
): Promise<RuntimeSettings> =>
  tauriInvoke("set_runtime_settings", {
    modelProfile: modelProfile ?? null,
    performanceProfile: performanceProfile ?? null,
    toggleShortcut: toggleShortcut ?? null,
    ortEp: ortEp ?? null,
    ortIntraThreads: ortIntraThreads ?? null,
    ortInterThreads: ortInterThreads ?? null,
    ortParallel: ortParallel ?? null,
    languageHint: languageHint ?? null,
    pillVisualizerSensitivity: pillVisualizerSensitivity ?? null,
    activitySensitivity: activitySensitivity ?? null,
    activityNoiseGate: activityNoiseGate ?? null,
    activityClipThreshold: activityClipThreshold ?? null,
    inputGainBoost: inputGainBoost ?? null,
    postUtteranceRefine: postUtteranceRefine ?? null,
    phraseBiasTerms: phraseBiasTerms ?? null,
    openAiApiKey: openAiApiKey ?? null,
    cloudMode: cloudMode ?? null,
    cloudOptIn: cloudOptIn ?? null,
    reliabilityMode: reliabilityMode ?? null,
    onboardingCompleted: onboardingCompleted ?? null,
    historyEnabled: historyEnabled ?? null,
    retentionDays: retentionDays ?? null,
  });

export const getPerfSnapshot = (): Promise<PerfSnapshot> =>
  tauriInvoke("get_perf_snapshot");

export const getPrivacySettings = (): Promise<PrivacySettings> =>
  tauriInvoke("get_privacy_settings");

export const setPrivacySettings = (
  historyEnabled?: boolean | null,
  retentionDays?: number | null,
  cloudOptIn?: boolean | null,
): Promise<PrivacySettings> =>
  tauriInvoke("set_privacy_settings", {
    historyEnabled: historyEnabled ?? null,
    retentionDays: retentionDays ?? null,
    cloudOptIn: cloudOptIn ?? null,
  });

export const getHistory = (
  page?: number,
  pageSize?: number,
  query?: string | null,
): Promise<HistoryPage> =>
  tauriInvoke("get_history", {
    page: page ?? null,
    pageSize: pageSize ?? null,
    query: query ?? null,
  });

export const deleteHistory = (
  ids?: string[] | null,
  olderThanDays?: number | null,
): Promise<number> =>
  tauriInvoke("delete_history", {
    ids: ids ?? null,
    olderThanDays: olderThanDays ?? null,
  });

export const getStats = (rangeDays?: number): Promise<StatsPayload> =>
  tauriInvoke("get_stats", {
    rangeDays: rangeDays ?? null,
  });

export const getDictionary = (): Promise<DictionaryEntry[]> =>
  tauriInvoke("get_dictionary");

export const upsertDictionary = (
  entry: DictionaryEntry,
): Promise<DictionaryEntry> =>
  tauriInvoke("upsert_dictionary", { entry });

export const deleteDictionary = (id: string): Promise<void> =>
  tauriInvoke("delete_dictionary", { id });

export const getSnippets = (): Promise<SnippetEntry[]> =>
  tauriInvoke("get_snippets");

export const upsertSnippet = (entry: SnippetEntry): Promise<SnippetEntry> =>
  tauriInvoke("upsert_snippet", { entry });

export const deleteSnippet = (id: string): Promise<void> =>
  tauriInvoke("delete_snippet", { id });

export const getLearnedCorrections = (): Promise<LearnedCorrection[]> =>
  tauriInvoke("get_learned_corrections");

export const learnCorrection = (
  heard: string,
  corrected: string,
): Promise<LearnedCorrection[]> =>
  tauriInvoke("learn_correction", {
    heard,
    corrected,
  });

export const deleteLearnedCorrection = (
  heard: string,
  corrected?: string | null,
): Promise<LearnedCorrection[]> =>
  tauriInvoke("delete_learned_correction", {
    heard,
    corrected: corrected ?? null,
  });

// ---------------------------------------------------------------------------
// Event listeners
// ---------------------------------------------------------------------------

export const TRANSCRIPT_EVENT = "dictum://transcript" as const;
export const STATUS_EVENT = "dictum://status" as const;
export const ACTIVITY_EVENT = "dictum://activity" as const;

export const listenTranscript = (
  handler: (event: TranscriptEvent) => void
): Promise<UnlistenFn> =>
  tauriListen<TranscriptEvent>(TRANSCRIPT_EVENT, (e) => handler(e.payload));

export const listenStatus = (
  handler: (event: EngineStatusEvent) => void
): Promise<UnlistenFn> =>
  tauriListen<EngineStatusEvent>(STATUS_EVENT, (e) => handler(e.payload));

export const listenActivity = (
  handler: (event: AudioActivityEvent) => void
): Promise<UnlistenFn> =>
  tauriListen<AudioActivityEvent>(ACTIVITY_EVENT, (e) => handler(e.payload));
