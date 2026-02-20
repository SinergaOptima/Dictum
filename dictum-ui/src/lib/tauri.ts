/**
 * Typed wrappers around @tauri-apps/api primitives.
 *
 * Centralises all Tauri IPC calls so command names and payload types are
 * defined in one place. TypeScript will catch mismatches at compile time.
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
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

export const setRuntimeSettings = (
  modelProfile?: string | null,
  ortEp?: string | null,
  languageHint?: string | null,
  cloudOptIn?: boolean | null,
  historyEnabled?: boolean | null,
  retentionDays?: number | null,
): Promise<RuntimeSettings> =>
  tauriInvoke("set_runtime_settings", {
    modelProfile: modelProfile ?? null,
    ortEp: ortEp ?? null,
    languageHint: languageHint ?? null,
    cloudOptIn: cloudOptIn ?? null,
    historyEnabled: historyEnabled ?? null,
    retentionDays: retentionDays ?? null,
  });

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
