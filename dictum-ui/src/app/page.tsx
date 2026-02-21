"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useActivity } from "@/hooks/useActivity";
import { useAudioDevices } from "@/hooks/useAudioDevices";
import { useEngine } from "@/hooks/useEngine";
import { useTranscript } from "@/hooks/useTranscript";
import type {
  DictionaryEntry,
  HistoryItem,
  PrivacySettings,
  SnippetEntry,
  StatsPayload,
} from "@shared/ipc_types";
import {
  deleteDictionary,
  deleteHistory,
  deleteSnippet,
  getDictionary,
  getHistory,
  getPreferredInputDevice,
  getPrivacySettings,
  getRuntimeSettings,
  getSnippets,
  getStats,
  setPreferredInputDevice,
  setRuntimeSettings,
  upsertDictionary,
  upsertSnippet,
} from "@/lib/tauri";

type Tab = "live" | "history" | "stats" | "dictionary" | "snippets" | "settings";

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return (
    target.isContentEditable ||
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT"
  );
}

const MODEL_PROFILE_OPTIONS = [
  { value: "large-v3-turbo", label: "Large v3 Turbo (default, fast + high quality)" },
  { value: "distil-large-v3", label: "Distil Large v3 (fast, English-optimized)" },
  { value: "large-v3", label: "Large v3 (max quality, heavier)" },
  { value: "medium.en", label: "Medium English" },
  { value: "medium", label: "Medium (multilingual)" },
  { value: "small.en", label: "Small English" },
  { value: "small", label: "Small (multilingual)" },
  { value: "base.en", label: "Base English" },
  { value: "base", label: "Base (multilingual)" },
  { value: "tiny.en", label: "Tiny English" },
  { value: "tiny", label: "Tiny (multilingual, fastest)" },
];

const ORT_EP_OPTIONS = [
  { value: "auto", label: "Auto" },
  { value: "directml", label: "DirectML (GPU)" },
  { value: "cpu", label: "CPU" },
];

const PERFORMANCE_PROFILE_OPTIONS = [
  { value: "whisper_balanced_english", label: "Whisper Balanced (English)" },
  { value: "stability_long_form", label: "Stability (Long Form)" },
  { value: "balanced_general", label: "Balanced" },
  { value: "latency_short_utterance", label: "Latency (Short Utterance)" },
];

const LANGUAGE_HINT_OPTIONS = [
  { value: "auto", label: "Auto" },
  { value: "english", label: "English" },
  { value: "mandarin", label: "Mandarin" },
  { value: "russian", label: "Russian" },
];

const MIC_CALIBRATION_STORAGE_KEY = "dictum-mic-calibration-v1";

type MicCalibration = {
  pillVisualizerSensitivity: number;
  activitySensitivity: number;
  activityNoiseGate: number;
  activityClipThreshold: number;
  inputGainBoost: number;
};

const clamp = (v: number, min: number, max: number): number => Math.min(max, Math.max(min, v));

const percentile = (samples: number[], p: number): number => {
  if (samples.length === 0) return 0;
  const sorted = [...samples].sort((a, b) => a - b);
  const idx = Math.floor((sorted.length - 1) * clamp(p, 0, 1));
  return sorted[idx] ?? 0;
};

export default function Home() {
  const { isListening, status, startEngine, stopEngine, error } = useEngine();
  const [activitySensitivity, setActivitySensitivity] = useState(4.2);
  const [activityNoiseGate, setActivityNoiseGate] = useState(0.0015);
  const [activityClipThreshold, setActivityClipThreshold] = useState(0.32);
  const { isSpeech, level, rawRms, isNoisy, isClipping } = useActivity({
    sensitivity: activitySensitivity,
    noiseGate: activityNoiseGate,
    clipThreshold: activityClipThreshold,
  });
  const { segments, clearSegments } = useTranscript();
  const { defaultDevice, devices, loading } = useAudioDevices();

  const feedRef = useRef<HTMLDivElement>(null);
  const pushToTalkRef = useRef(false);
  const latestRmsRef = useRef(0);

  const [tab, setTab] = useState<Tab>("live");
  const [copyState, setCopyState] = useState<"idle" | "done" | "error">("idle");
  const [selectedDeviceName, setSelectedDeviceName] = useState<string | null>(null);
  const [modelProfile, setModelProfile] = useState("large-v3-turbo");
  const [performanceProfile, setPerformanceProfile] = useState("whisper_balanced_english");
  const [toggleShortcut, setToggleShortcut] = useState("Ctrl+Shift+Space");
  const [ortEp, setOrtEp] = useState("auto");
  const [languageHint, setLanguageHint] = useState("english");
  const [pillVisualizerSensitivity, setPillVisualizerSensitivity] = useState(10);
  const [inputGainBoost, setInputGainBoost] = useState(1);
  const [postUtteranceRefine, setPostUtteranceRefine] = useState(false);
  const [phraseBiasTerms, setPhraseBiasTerms] = useState("");
  const [openAiApiKeyInput, setOpenAiApiKeyInput] = useState("");
  const [hasOpenAiApiKey, setHasOpenAiApiKey] = useState(false);
  const [cloudOptIn, setCloudOptIn] = useState(false);
  const [historyEnabled, setHistoryEnabled] = useState(true);
  const [retentionDays, setRetentionDays] = useState(90);
  const [runtimeMsg, setRuntimeMsg] = useState<string | null>(null);
  const [calibrationMsg, setCalibrationMsg] = useState<string | null>(null);
  const [isCalibrating, setIsCalibrating] = useState(false);

  const [historyItems, setHistoryItems] = useState<HistoryItem[]>([]);
  const [historyQuery, setHistoryQuery] = useState("");
  const [stats, setStats] = useState<StatsPayload | null>(null);
  const [dictionary, setDictionary] = useState<DictionaryEntry[]>([]);
  const [snippets, setSnippets] = useState<SnippetEntry[]>([]);
  const [dictTerm, setDictTerm] = useState("");
  const [dictAliases, setDictAliases] = useState("");
  const [dictLanguage, setDictLanguage] = useState("");
  const [snippetTrigger, setSnippetTrigger] = useState("");
  const [snippetExpansion, setSnippetExpansion] = useState("");
  const [snippetMode, setSnippetMode] = useState<"slash" | "phrase">("slash");

  const copyText = useMemo(() => {
    const finals = segments.filter((seg) => seg.kind === "final");
    const source = finals.length > 0 ? finals : segments;
    return source.map((seg) => seg.text.trim()).filter(Boolean).join("\n");
  }, [segments]);

  useEffect(() => {
    if (feedRef.current) {
      feedRef.current.scrollTop = feedRef.current.scrollHeight;
    }
  }, [segments]);

  useEffect(() => {
    latestRmsRef.current = rawRms;
  }, [rawRms]);

  useEffect(() => {
    getPreferredInputDevice()
      .then((name) => setSelectedDeviceName(name))
      .catch((err) => console.warn("Could not fetch preferred input device:", err));
  }, []);

  useEffect(() => {
    Promise.all([getRuntimeSettings(), getPrivacySettings()])
      .then(([runtime, privacy]) => {
        setModelProfile(runtime.modelProfile || "large-v3-turbo");
        setPerformanceProfile(runtime.performanceProfile || "whisper_balanced_english");
        setToggleShortcut(runtime.toggleShortcut || "Ctrl+Shift+Space");
        setOrtEp(runtime.ortEp || "auto");
        setLanguageHint(runtime.languageHint || "english");
        setPillVisualizerSensitivity(runtime.pillVisualizerSensitivity || 10);
        setActivitySensitivity(runtime.activitySensitivity || 4.2);
        setActivityNoiseGate(runtime.activityNoiseGate ?? 0.0015);
        setActivityClipThreshold(runtime.activityClipThreshold ?? 0.32);
        setInputGainBoost(runtime.inputGainBoost || 1);
        setPostUtteranceRefine(runtime.postUtteranceRefine ?? false);
        setPhraseBiasTerms((runtime.phraseBiasTerms || []).join("\n"));
        setHasOpenAiApiKey(runtime.hasOpenAiApiKey ?? false);
        setCloudOptIn(privacy.cloudOptIn);
        setHistoryEnabled(privacy.historyEnabled);
        setRetentionDays(privacy.retentionDays ?? 90);
      })
      .catch((err) => console.warn("Could not fetch runtime/privacy settings:", err));
  }, []);

  useEffect(() => {
    if (performanceProfile === "whisper_balanced_english" && languageHint !== "english") {
      setLanguageHint("english");
    }
  }, [performanceProfile, languageHint]);

  useEffect(() => {
    if (!selectedDeviceName && defaultDevice?.name) {
      setSelectedDeviceName(defaultDevice.name);
    }
  }, [defaultDevice, selectedDeviceName]);

  useEffect(() => {
    if (!selectedDeviceName) return;
    try {
      const raw = localStorage.getItem(MIC_CALIBRATION_STORAGE_KEY);
      if (!raw) return;
      const all = JSON.parse(raw) as Record<string, MicCalibration>;
      const profile = all[selectedDeviceName];
      if (!profile) return;
      setPillVisualizerSensitivity(profile.pillVisualizerSensitivity);
      setActivitySensitivity(profile.activitySensitivity);
      setActivityNoiseGate(profile.activityNoiseGate);
      setActivityClipThreshold(profile.activityClipThreshold);
      setInputGainBoost(profile.inputGainBoost);
      void applyRuntime({
        pillVisualizerSensitivity: profile.pillVisualizerSensitivity,
        activitySensitivity: profile.activitySensitivity,
        activityNoiseGate: profile.activityNoiseGate,
        activityClipThreshold: profile.activityClipThreshold,
        inputGainBoost: profile.inputGainBoost,
      });
    } catch {
      // Ignore malformed local calibration cache.
    }
  }, [selectedDeviceName]);

  useEffect(() => {
    if (copyState === "idle") return;
    const timer = window.setTimeout(() => setCopyState("idle"), 1400);
    return () => window.clearTimeout(timer);
  }, [copyState]);

  const copyTranscript = useCallback(async () => {
    if (!copyText) return;
    try {
      await navigator.clipboard.writeText(copyText);
      setCopyState("done");
    } catch {
      setCopyState("error");
    }
  }, [copyText]);

  const handleToggle = useCallback(async () => {
    if (isListening) {
      await stopEngine();
    } else {
      await startEngine(selectedDeviceName);
    }
  }, [isListening, startEngine, stopEngine, selectedDeviceName]);

  const applyRuntime = useCallback(
    async (overrides?: Partial<{
      modelProfile: string;
      performanceProfile: string;
      toggleShortcut: string;
      ortEp: string;
      languageHint: string;
      pillVisualizerSensitivity: number;
      activitySensitivity: number;
      activityNoiseGate: number;
      activityClipThreshold: number;
      inputGainBoost: number;
      postUtteranceRefine: boolean;
      phraseBiasTerms: string[];
      openAiApiKey: string | null;
      cloudOptIn: boolean;
      historyEnabled: boolean;
      retentionDays: number;
    }>) => {
      const next = {
        modelProfile,
        performanceProfile,
        toggleShortcut,
        ortEp,
        languageHint,
        pillVisualizerSensitivity,
        activitySensitivity,
        activityNoiseGate,
        activityClipThreshold,
        inputGainBoost,
        postUtteranceRefine,
        phraseBiasTerms: phraseBiasTerms
          .split(/\r?\n|,/)
          .map((t) => t.trim())
          .filter(Boolean),
        openAiApiKey: openAiApiKeyInput.trim() ? openAiApiKeyInput.trim() : null,
        cloudOptIn,
        historyEnabled,
        retentionDays,
        ...overrides,
      };
      if (next.performanceProfile === "whisper_balanced_english") {
        next.languageHint = "english";
      }
      try {
        const updated = await setRuntimeSettings(
          next.modelProfile,
          next.performanceProfile,
          next.toggleShortcut,
          next.ortEp,
          next.languageHint,
          next.pillVisualizerSensitivity,
          next.activitySensitivity,
          next.activityNoiseGate,
          next.activityClipThreshold,
          next.inputGainBoost,
          next.postUtteranceRefine,
          next.phraseBiasTerms,
          next.openAiApiKey,
          next.cloudOptIn,
          next.historyEnabled,
          next.retentionDays,
        );
        setModelProfile(updated.modelProfile || "large-v3-turbo");
        setPerformanceProfile(updated.performanceProfile || "whisper_balanced_english");
        setToggleShortcut(updated.toggleShortcut || "Ctrl+Shift+Space");
        setOrtEp(updated.ortEp || "auto");
        setLanguageHint(updated.languageHint || "english");
        setPillVisualizerSensitivity(updated.pillVisualizerSensitivity || 10);
        setActivitySensitivity(updated.activitySensitivity || 4.2);
        setActivityNoiseGate(updated.activityNoiseGate ?? 0.0015);
        setActivityClipThreshold(updated.activityClipThreshold ?? 0.32);
        setInputGainBoost(updated.inputGainBoost || 1);
        setPostUtteranceRefine(updated.postUtteranceRefine ?? false);
        setPhraseBiasTerms((updated.phraseBiasTerms || []).join("\n"));
        setHasOpenAiApiKey(updated.hasOpenAiApiKey ?? false);
        if (next.openAiApiKey !== null) {
          setOpenAiApiKeyInput("");
        }
        setCloudOptIn(updated.cloudOptIn);
        setHistoryEnabled(updated.historyEnabled);
        setRetentionDays(updated.retentionDays);
        if (updated.cloudOptIn && !updated.hasOpenAiApiKey) {
          setRuntimeMsg("Saved. Add an OpenAI API key to use cloud fallback.");
        } else {
          setRuntimeMsg("Saved.");
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setRuntimeMsg(`Failed to save settings: ${msg}`);
      }
    },
    [
      modelProfile,
      performanceProfile,
      toggleShortcut,
      ortEp,
      languageHint,
      pillVisualizerSensitivity,
      activitySensitivity,
      activityNoiseGate,
      activityClipThreshold,
      inputGainBoost,
      postUtteranceRefine,
      phraseBiasTerms,
      openAiApiKeyInput,
      cloudOptIn,
      historyEnabled,
      retentionDays,
    ],
  );

  const collectRmsSamples = useCallback(async (durationMs: number): Promise<number[]> => {
    const startedAt = performance.now();
    const samples: number[] = [];
    await new Promise<void>((resolve) => {
      const timer = window.setInterval(() => {
        samples.push(latestRmsRef.current);
        if (performance.now() - startedAt >= durationMs) {
          window.clearInterval(timer);
          resolve();
        }
      }, 45);
    });
    return samples;
  }, []);

  const runMicCalibration = useCallback(async () => {
    if (isCalibrating) return;
    setIsCalibrating(true);
    setCalibrationMsg("Ambient phase: stay silent...");
    const startedByCalibration = !isListening;
    try {
      if (!isListening) {
        await startEngine(selectedDeviceName);
        await new Promise((r) => window.setTimeout(r, 450));
      }
      const ambient = await collectRmsSamples(2200);
      setCalibrationMsg("Whisper phase: whisper naturally...");
      await new Promise((r) => window.setTimeout(r, 250));
      const whisper = await collectRmsSamples(2600);

      const ambientP90 = percentile(ambient, 0.9);
      const whisperP70 = percentile(whisper, 0.7);
      const recommendedNoiseGate = clamp(ambientP90 * 1.45, 0.0004, 0.03);
      const recommendedActivitySensitivity = clamp(
        0.34 / Math.max(0.0001, whisperP70 - recommendedNoiseGate),
        1.0,
        20.0,
      );
      const recommendedPillSensitivity = clamp(recommendedActivitySensitivity * 1.12, 1.0, 20.0);
      const recommendedInputGainBoost = clamp(0.02 / Math.max(0.0001, whisperP70), 0.5, 8.0);

      setActivityNoiseGate(recommendedNoiseGate);
      setActivitySensitivity(recommendedActivitySensitivity);
      setActivityClipThreshold(clamp(Math.max(ambientP90 * 12, whisperP70 * 8), 0.12, 0.95));
      setPillVisualizerSensitivity(recommendedPillSensitivity);
      setInputGainBoost(recommendedInputGainBoost);

      if (selectedDeviceName) {
        try {
          const raw = localStorage.getItem(MIC_CALIBRATION_STORAGE_KEY);
          const all = (raw ? JSON.parse(raw) : {}) as Record<string, MicCalibration>;
          all[selectedDeviceName] = {
            pillVisualizerSensitivity: recommendedPillSensitivity,
            activitySensitivity: recommendedActivitySensitivity,
            activityNoiseGate: recommendedNoiseGate,
            activityClipThreshold: clamp(Math.max(ambientP90 * 12, whisperP70 * 8), 0.12, 0.95),
            inputGainBoost: recommendedInputGainBoost,
          };
          localStorage.setItem(MIC_CALIBRATION_STORAGE_KEY, JSON.stringify(all));
        } catch {
          // Ignore local storage errors.
        }
      }

      await applyRuntime({
        pillVisualizerSensitivity: recommendedPillSensitivity,
        activitySensitivity: recommendedActivitySensitivity,
        activityNoiseGate: recommendedNoiseGate,
        activityClipThreshold: clamp(Math.max(ambientP90 * 12, whisperP70 * 8), 0.12, 0.95),
        inputGainBoost: recommendedInputGainBoost,
      });
      setCalibrationMsg("Calibration complete and applied.");
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setCalibrationMsg(`Calibration failed: ${msg}`);
    } finally {
      if (startedByCalibration) {
        await stopEngine().catch(() => undefined);
      }
      setIsCalibrating(false);
    }
  }, [
    isCalibrating,
    isListening,
    selectedDeviceName,
    startEngine,
    collectRmsSamples,
    applyRuntime,
    stopEngine,
  ]);

  const refreshHistory = useCallback(async () => {
    const page = await getHistory(1, 100, historyQuery || null);
    setHistoryItems(page.items);
  }, [historyQuery]);

  const refreshStats = useCallback(async () => {
    const data = await getStats(30);
    setStats(data);
  }, []);

  const refreshDictionary = useCallback(async () => {
    setDictionary(await getDictionary());
  }, []);

  const refreshSnippets = useCallback(async () => {
    setSnippets(await getSnippets());
  }, []);

  useEffect(() => {
    if (tab === "history") void refreshHistory();
    if (tab === "stats") void refreshStats();
    if (tab === "dictionary") void refreshDictionary();
    if (tab === "snippets") void refreshSnippets();
  }, [tab, refreshHistory, refreshStats, refreshDictionary, refreshSnippets]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const isMod = event.ctrlKey || event.metaKey;

      if (isMod && event.key.toLowerCase() === "enter") {
        if (!isEditableTarget(event.target)) {
          event.preventDefault();
          void handleToggle();
        }
        return;
      }

      if (isMod && event.shiftKey && event.key.toLowerCase() === "c") {
        if (!isEditableTarget(event.target)) {
          event.preventDefault();
          void copyTranscript();
        }
        return;
      }

      if (isMod && event.key.toLowerCase() === "l") {
        if (!isEditableTarget(event.target)) {
          event.preventDefault();
          clearSegments();
        }
        return;
      }

      if (event.code !== "Space" || event.repeat || isEditableTarget(event.target)) return;
      event.preventDefault();
      if (isMod || event.altKey) return;
      if (pushToTalkRef.current || isListening) return;

      pushToTalkRef.current = true;
      void startEngine(selectedDeviceName).catch(() => {
        pushToTalkRef.current = false;
      });
    };

    const onKeyUp = (event: KeyboardEvent) => {
      if (event.code !== "Space" || !pushToTalkRef.current) return;
      event.preventDefault();
      pushToTalkRef.current = false;
      if (isListening) void stopEngine();
    };

    const onBlur = () => {
      if (!pushToTalkRef.current) return;
      pushToTalkRef.current = false;
      if (isListening) void stopEngine();
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
    };
  }, [clearSegments, copyTranscript, handleToggle, isListening, selectedDeviceName, startEngine, stopEngine]);

  const statusLabel =
    status === "listening" && isSpeech ? "Hearing" :
    status === "listening" ? "Listening" :
    status === "warmingup" ? "Loading" :
    status === "idle" ? "Ready" :
    status === "stopped" ? "Stopped" :
    status === "error" ? "Error" :
    status;

  const hasSegments = segments.length > 0;
  const rmsMeterPercent = clamp(rawRms * 1600, 0, 1) * 100;
  const noiseGatePercent = clamp(activityNoiseGate / 0.03, 0, 1) * 100;
  const clipThresholdPercent = clamp(activityClipThreshold, 0, 1) * 100;

  return (
    <div className="app-layout">
      <div className="theme-bg" aria-hidden />

      <header className="app-header">
        <span className="app-brand">Dictum</span>
        <span
          className={`app-status-badge${isListening ? " is-listening" : ""}${status === "error" ? " is-error" : ""}`}
          role="status"
          aria-live="polite"
        >
          {statusLabel}
        </span>
        <div className="tabs-row">
          {(["live", "history", "stats", "dictionary", "snippets", "settings"] as Tab[]).map((value) => (
            <button
              key={value}
              className={`tab-btn${tab === value ? " active" : ""}`}
              onClick={() => setTab(value)}
              data-no-drag
            >
              {value}
            </button>
          ))}
        </div>
        <div className="app-spacer" />
        {error && (
          <span className="error-banner" role="alert" title={error}>
            {error}
          </span>
        )}
      </header>

      {tab === "live" && (
        <>
          <div className="transcript-scroll selectable" ref={feedRef}>
            {hasSegments ? (
              <div className="transcript-feed">
                {segments.map((seg) => (
                  <p key={seg.id} className={seg.kind === "partial" ? "seg-partial" : "seg-final"}>
                    {seg.text}
                  </p>
                ))}
              </div>
            ) : (
              <div className="empty-state">
                <div className="empty-glyph" aria-hidden>D</div>
                <p className="empty-label">
                  {isListening ? "Listening for speech..." : "Your transcript will appear here"}
                </p>
                <p className="empty-hint">Hold Space · Ctrl+Enter · Ctrl+Shift+Space</p>
              </div>
            )}
          </div>

          <footer className="app-footer">
            <div className="device-select-wrap">
              <svg className="mic-icon" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden>
                <rect x="5.5" y="1" width="5" height="8" rx="2.5" stroke="currentColor" strokeWidth="1.4" />
                <path d="M3 8a5 5 0 0010 0" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
                <line x1="8" y1="13" x2="8" y2="15" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
              </svg>
              <select
                className="device-select"
                value={selectedDeviceName ?? ""}
                onChange={(e) => {
                  const next = e.target.value || null;
                  setSelectedDeviceName(next);
                  void setPreferredInputDevice(next).catch(console.error);
                }}
                disabled={isListening || loading}
                aria-label="Microphone input device"
              >
                {selectedDeviceName && !devices.some((d) => d.name === selectedDeviceName) && (
                  <option value={selectedDeviceName}>{selectedDeviceName} (unavailable)</option>
                )}
                {devices.map((d) => (
                  <option key={d.name} value={d.name}>
                    {d.name}{d.isDefault ? " *" : ""}
                  </option>
                ))}
              </select>
            </div>

            <div className="footer-spacer" />

            {hasSegments && (
              <>
                <button type="button" className="action-btn" onClick={() => void copyTranscript()} disabled={!copyText}>
                  {copyState === "done" ? "Copied" : copyState === "error" ? "Failed" : "Copy"}
                </button>
                <button type="button" className="action-btn" onClick={clearSegments}>
                  Clear
                </button>
              </>
            )}

            {isListening && (
              <div className="level-bars" aria-hidden>
                {Array.from({ length: 7 }).map((_, i) => {
                  const active = i < Math.ceil(level * 7);
                  return (
                    <span
                      key={i}
                      className={`level-bar${active ? " active" : ""}`}
                      style={active ? { height: `${4 + level * 13 + i}px` } : undefined}
                    />
                  );
                })}
              </div>
            )}

            <button
              type="button"
              className={`record-btn${isListening ? " record-btn--live" : ""}`}
              onClick={() => void handleToggle()}
              aria-pressed={isListening}
            >
              {isListening ? <span className="record-stop" aria-hidden /> : <span className="record-dot" aria-hidden />}
            </button>
          </footer>
        </>
      )}

      {tab !== "live" && (
        <div className="panel-scroll selectable" data-no-drag>
          {tab === "history" && (
            <section className="panel">
              <div className="panel-toolbar">
                <input
                  className="runtime-select panel-input"
                  placeholder="Search dictation history..."
                  value={historyQuery}
                  onChange={(e) => setHistoryQuery(e.target.value)}
                />
                <button className="action-btn" onClick={() => void refreshHistory()}>Refresh</button>
                <button
                  className="action-btn"
                  onClick={async () => {
                    await deleteHistory(null, retentionDays);
                    await refreshHistory();
                  }}
                >
                  Prune
                </button>
              </div>
              <div className="panel-list">
                {historyItems.map((item) => (
                  <article key={item.id} className="panel-card">
                    <div className="panel-meta">
                      <span>{new Date(item.createdAt).toLocaleString()}</span>
                      <span>{item.source}</span>
                      <span>{item.wordCount} words</span>
                    </div>
                    <p>{item.text}</p>
                    <button
                      className="action-btn"
                      onClick={async () => {
                        await deleteHistory([item.id], null);
                        await refreshHistory();
                      }}
                    >
                      Delete
                    </button>
                  </article>
                ))}
                {historyItems.length === 0 && <p className="empty-label">No history yet.</p>}
              </div>
            </section>
          )}

          {tab === "stats" && (
            <section className="panel">
              <div className="panel-toolbar">
                <button className="action-btn" onClick={() => void refreshStats()}>Refresh</button>
              </div>
              {stats ? (
                <div className="panel-grid">
                  <div className="stat-card"><b>{stats.totalUtterances}</b><span>Utterances</span></div>
                  <div className="stat-card"><b>{stats.totalWords}</b><span>Words</span></div>
                  <div className="stat-card"><b>{Math.round(stats.avgLatencyMs)} ms</b><span>Avg Latency</span></div>
                </div>
              ) : <p className="empty-label">No stats yet.</p>}
            </section>
          )}

          {tab === "dictionary" && (
            <section className="panel">
              <div className="panel-toolbar">
                <input className="runtime-select panel-input" placeholder="Canonical term" value={dictTerm} onChange={(e) => setDictTerm(e.target.value)} />
                <input className="runtime-select panel-input" placeholder="Aliases (comma-separated)" value={dictAliases} onChange={(e) => setDictAliases(e.target.value)} />
                <input className="runtime-select panel-input" placeholder="Language (optional)" value={dictLanguage} onChange={(e) => setDictLanguage(e.target.value)} />
                <button
                  className="action-btn"
                  onClick={async () => {
                    if (!dictTerm.trim()) return;
                    await upsertDictionary({
                      id: "",
                      term: dictTerm.trim(),
                      aliases: dictAliases.split(",").map((v) => v.trim()).filter(Boolean),
                      language: dictLanguage.trim() || null,
                      enabled: true,
                      createdAt: "",
                      updatedAt: "",
                    });
                    setDictTerm("");
                    setDictAliases("");
                    setDictLanguage("");
                    await refreshDictionary();
                  }}
                >
                  Add
                </button>
              </div>
              <div className="panel-list">
                {dictionary.map((entry) => (
                  <article key={entry.id} className="panel-card">
                    <div className="panel-meta">
                      <span>{entry.term}</span>
                      <span>{entry.language ?? "any"}</span>
                    </div>
                    <p>{entry.aliases.join(", ") || "No aliases"}</p>
                    <button className="action-btn" onClick={async () => { await deleteDictionary(entry.id); await refreshDictionary(); }}>Delete</button>
                  </article>
                ))}
                {dictionary.length === 0 && <p className="empty-label">No dictionary entries yet.</p>}
              </div>
            </section>
          )}

          {tab === "snippets" && (
            <section className="panel">
              <div className="panel-toolbar">
                <input className="runtime-select panel-input" placeholder="Trigger (e.g. /email)" value={snippetTrigger} onChange={(e) => setSnippetTrigger(e.target.value)} />
                <input className="runtime-select panel-input" placeholder="Expansion text" value={snippetExpansion} onChange={(e) => setSnippetExpansion(e.target.value)} />
                <select className="runtime-select" value={snippetMode} onChange={(e) => setSnippetMode(e.target.value as "slash" | "phrase")}>
                  <option value="slash">slash</option>
                  <option value="phrase">phrase</option>
                </select>
                <button
                  className="action-btn"
                  onClick={async () => {
                    if (!snippetTrigger.trim() || !snippetExpansion.trim()) return;
                    await upsertSnippet({
                      id: "",
                      trigger: snippetTrigger.trim(),
                      expansion: snippetExpansion.trim(),
                      mode: snippetMode,
                      enabled: true,
                      createdAt: "",
                      updatedAt: "",
                    });
                    setSnippetTrigger("");
                    setSnippetExpansion("");
                    await refreshSnippets();
                  }}
                >
                  Add
                </button>
              </div>
              <div className="panel-list">
                {snippets.map((entry) => (
                  <article key={entry.id} className="panel-card">
                    <div className="panel-meta">
                      <span>{entry.trigger}</span>
                      <span>{entry.mode}</span>
                    </div>
                    <p>{entry.expansion}</p>
                    <button className="action-btn" onClick={async () => { await deleteSnippet(entry.id); await refreshSnippets(); }}>Delete</button>
                  </article>
                ))}
                {snippets.length === 0 && <p className="empty-label">No snippets yet.</p>}
              </div>
            </section>
          )}

          {tab === "settings" && (
            <section className="settings-shell">
              <header className="settings-hero">
                <div className="settings-hero-copy">
                  <span className="settings-kicker">Workspace</span>
                  <h2>Runtime Settings</h2>
                  <p>Tune recognition quality, latency, and voice behavior with live feedback.</p>
                </div>
                <div className="settings-hero-actions">
                  <button className="action-btn" onClick={() => void runMicCalibration()} disabled={isCalibrating}>
                    {isCalibrating ? "Calibrating..." : "Calibrate Mic"}
                  </button>
                  <button className="action-btn settings-save-btn" onClick={() => void applyRuntime()}>
                    Save Settings
                  </button>
                </div>
              </header>

              <div className="settings-health-row">
                <article className="settings-health-chip">
                  <span>Active Mic</span>
                  <strong>{selectedDeviceName ?? (loading ? "Detecting..." : "System Default")}</strong>
                </article>
                <article className="settings-health-chip">
                  <span>Voice Activity</span>
                  <strong>{isSpeech ? "Speech Detected" : "Listening for Speech"}</strong>
                </article>
                <article className="settings-health-chip">
                  <span>Live RMS</span>
                  <strong>{rawRms.toFixed(4)}</strong>
                </article>
              </div>

              <article className="settings-card settings-card-full">
                <div className="settings-card-header">
                  <h3>Quick Profiles</h3>
                  <p>Fast presets for common environments. You can fine-tune afterward.</p>
                </div>
                <div className="settings-preset-grid">
                  <button
                    className="settings-preset-btn"
                    onClick={() => {
                      setPerformanceProfile("latency_short_utterance");
                      setPillVisualizerSensitivity(18);
                      setActivitySensitivity(12);
                      setActivityNoiseGate(0.0008);
                      setActivityClipThreshold(0.28);
                      setInputGainBoost(3.2);
                    }}
                  >
                    <span>Whisper Catch</span>
                    <small>Maximum pickup for quiet speech.</small>
                  </button>
                  <button
                    className="settings-preset-btn"
                    onClick={() => {
                      setPerformanceProfile("balanced_general");
                      setPillVisualizerSensitivity(12);
                      setActivitySensitivity(7.5);
                      setActivityNoiseGate(0.0015);
                      setActivityClipThreshold(0.32);
                      setInputGainBoost(1.8);
                    }}
                  >
                    <span>Balanced</span>
                    <small>Everyday dictation with stable detection.</small>
                  </button>
                  <button
                    className="settings-preset-btn"
                    onClick={() => {
                      setPerformanceProfile("stability_long_form");
                      setPillVisualizerSensitivity(9);
                      setActivitySensitivity(5.2);
                      setActivityNoiseGate(0.0028);
                      setActivityClipThreshold(0.4);
                      setInputGainBoost(1.1);
                    }}
                  >
                    <span>Noisy Room</span>
                    <small>More conservative gating for background noise.</small>
                  </button>
                </div>
              </article>

              <div className="settings-grid">
                <article className="settings-card">
                  <div className="settings-card-header">
                    <h3>Core</h3>
                    <p>Runtime model, language, and global controls.</p>
                  </div>
                  <div className="settings-fields">
                    <label className="settings-field">
                      <span>Model</span>
                      <select className="settings-input" value={modelProfile} onChange={(e) => setModelProfile(e.target.value)}>
                        {MODEL_PROFILE_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
                      </select>
                    </label>
                    <label className="settings-field">
                      <span>Runtime</span>
                      <select className="settings-input" value={ortEp} onChange={(e) => setOrtEp(e.target.value)}>
                        {ORT_EP_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
                      </select>
                    </label>
                    <label className="settings-field">
                      <span>Performance Profile</span>
                      <select className="settings-input" value={performanceProfile} onChange={(e) => setPerformanceProfile(e.target.value)}>
                        {PERFORMANCE_PROFILE_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
                      </select>
                    </label>
                    <label className="settings-field">
                      <span>Language</span>
                      <select
                        className="settings-input"
                        value={languageHint}
                        disabled={performanceProfile === "whisper_balanced_english"}
                        onChange={(e) => setLanguageHint(e.target.value)}
                      >
                        {LANGUAGE_HINT_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
                      </select>
                    </label>
                    <label className="settings-field">
                      <span>Toggle Shortcut</span>
                      <input
                        className="settings-input"
                        value={toggleShortcut}
                        onChange={(e) => setToggleShortcut(e.target.value)}
                        placeholder="Ctrl+Shift+Space"
                      />
                    </label>
                    <div className="settings-field">
                      <span>Shortcut Presets</span>
                      <div className="settings-chip-row">
                        <button className="settings-chip-btn" onClick={() => setToggleShortcut("Ctrl+Shift+Space")}>Ctrl+Shift+Space</button>
                        <button className="settings-chip-btn" onClick={() => setToggleShortcut("Ctrl+Alt+Space")}>Ctrl+Alt+Space</button>
                        <button className="settings-chip-btn" onClick={() => setToggleShortcut("Ctrl+Enter")}>Ctrl+Enter</button>
                      </div>
                    </div>
                    <label className="settings-field">
                      <span>Retention (days)</span>
                      <input
                        className="settings-input"
                        type="number"
                        min={1}
                        max={3650}
                        value={retentionDays}
                        onChange={(e) => setRetentionDays(Number(e.target.value || 90))}
                      />
                    </label>
                  </div>
                </article>

                <article className="settings-card">
                  <div className="settings-card-header">
                    <h3>Audio Intelligence</h3>
                    <p>Sensitivity and gating controls with live metering.</p>
                  </div>
                  <div className="settings-meter-stack">
                    <div className="settings-meter-row">
                      <span>Input level</span>
                      <div className="settings-meter">
                        <div style={{ width: `${rmsMeterPercent}%` }} />
                      </div>
                    </div>
                    <div className="settings-meter-row">
                      <span>Noise gate</span>
                      <div className="settings-meter is-gate">
                        <div style={{ width: `${noiseGatePercent}%` }} />
                      </div>
                    </div>
                    <div className="settings-meter-row">
                      <span>Clip threshold</span>
                      <div className="settings-meter is-clip">
                        <div style={{ width: `${clipThresholdPercent}%` }} />
                      </div>
                    </div>
                  </div>
                  <div className="settings-slider-group">
                    <label className="settings-slider">
                      <div>
                        <span>Pill Sensitivity</span>
                        <b>{pillVisualizerSensitivity.toFixed(1)}x</b>
                      </div>
                      <input type="range" min={1} max={20} step={0.1} value={pillVisualizerSensitivity} onChange={(e) => setPillVisualizerSensitivity(Number(e.target.value))} />
                    </label>
                    <label className="settings-slider">
                      <div>
                        <span>Activity Sensitivity</span>
                        <b>{activitySensitivity.toFixed(1)}x</b>
                      </div>
                      <input type="range" min={1} max={20} step={0.1} value={activitySensitivity} onChange={(e) => setActivitySensitivity(Number(e.target.value))} />
                    </label>
                    <label className="settings-slider">
                      <div>
                        <span>Noise Gate</span>
                        <b>{activityNoiseGate.toFixed(4)}</b>
                      </div>
                      <input type="range" min={0} max={0.1} step={0.0001} value={activityNoiseGate} onChange={(e) => setActivityNoiseGate(Number(e.target.value))} />
                    </label>
                    <label className="settings-slider">
                      <div>
                        <span>Clip Threshold</span>
                        <b>{activityClipThreshold.toFixed(2)}</b>
                      </div>
                      <input type="range" min={0.02} max={1} step={0.01} value={activityClipThreshold} onChange={(e) => setActivityClipThreshold(Number(e.target.value))} />
                    </label>
                    <label className="settings-slider">
                      <div>
                        <span>Input Gain Boost</span>
                        <b>{inputGainBoost.toFixed(1)}x</b>
                      </div>
                      <input type="range" min={0.5} max={8} step={0.1} value={inputGainBoost} onChange={(e) => setInputGainBoost(Number(e.target.value))} />
                    </label>
                  </div>
                  <div className="settings-inline-stats">
                    <span>Live RMS {rawRms.toFixed(4)}</span>
                    <span>{isNoisy ? "Noisy environment" : "Noise stable"}</span>
                    <span>{isClipping ? "Clipping risk" : "Headroom OK"}</span>
                  </div>
                </article>

                <article className="settings-card">
                  <div className="settings-card-header">
                    <h3>Recognition</h3>
                    <p>Bias and post-processing behavior for final transcript quality.</p>
                  </div>
                  <label className="settings-field">
                    <span>Phrase Bias Terms</span>
                    <textarea
                      className="settings-input settings-textarea"
                      value={phraseBiasTerms}
                      onChange={(e) => setPhraseBiasTerms(e.target.value)}
                      placeholder={"One term per line.\nLattice Labs\nDictum"}
                      rows={5}
                    />
                  </label>
                  <label className="settings-switch">
                    <input type="checkbox" checked={postUtteranceRefine} onChange={(e) => setPostUtteranceRefine(e.target.checked)} />
                    <span>Enable post-utterance refinement</span>
                  </label>
                </article>

                <article className="settings-card">
                  <div className="settings-card-header">
                    <h3>Privacy</h3>
                    <p>Control local retention and cloud fallback behavior.</p>
                  </div>
                  <label className="settings-field">
                    <span>OpenAI API Key</span>
                    <input
                      className="settings-input"
                      type="password"
                      value={openAiApiKeyInput}
                      onChange={(e) => setOpenAiApiKeyInput(e.target.value)}
                      placeholder={hasOpenAiApiKey ? "Saved locally. Enter new key to replace." : "sk-proj-..."}
                      autoComplete="off"
                    />
                  </label>
                  <div className="settings-inline-actions">
                    <button
                      className="action-btn"
                      disabled={!hasOpenAiApiKey}
                      onClick={() => void applyRuntime({ openAiApiKey: "" })}
                    >
                      Clear Saved Key
                    </button>
                    <span className="settings-note">
                      {hasOpenAiApiKey ? "API key saved locally for this profile." : "No API key saved yet."}
                    </span>
                  </div>
                  <label className="settings-switch">
                    <input type="checkbox" checked={cloudOptIn} onChange={(e) => setCloudOptIn(e.target.checked)} />
                    <span>Cloud fallback (OpenAI)</span>
                  </label>
                  <label className="settings-switch">
                    <input type="checkbox" checked={historyEnabled} onChange={(e) => setHistoryEnabled(e.target.checked)} />
                    <span>Save local history</span>
                  </label>
                  <p className="settings-note">
                    {runtimeMsg ?? "Cloud fallback requires an OpenAI API key."}
                  </p>
                  <p className="settings-note">
                    {calibrationMsg ?? "Run mic calibration after changing devices or recording environment."}
                  </p>
                </article>
              </div>
            </section>
          )}
        </div>
      )}
    </div>
  );
}
