"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useActivity } from "@/hooks/useActivity";
import { useAudioDevices } from "@/hooks/useAudioDevices";
import { useEngine } from "@/hooks/useEngine";
import { useTranscript } from "@/hooks/useTranscript";
import type {
  AppUpdateInfo,
  DictionaryEntry,
  HistoryItem,
  LearnedCorrection,
  ModelProfileMetadata,
  ModelProfileRecommendation,
  PerfSnapshot,
  PrivacySettings,
  SnippetEntry,
  StatsPayload,
} from "@shared/ipc_types";
import {
  deleteLearnedCorrection,
  deleteDictionary,
  deleteHistory,
  deleteSnippet,
  getDictionary,
  getHistory,
  getLearnedCorrections,
  getModelProfileCatalog,
  getModelProfileRecommendation,
  getPerfSnapshot,
  getPreferredInputDevice,
  getPrivacySettings,
  getAppVersion,
  getRuntimeSettings,
  getSnippets,
  getStats,
  learnCorrection,
  runAutoTune,
  runBenchmarkAutoTune,
  checkForAppUpdate,
  downloadAndInstallAppUpdate,
  setPreferredInputDevice,
  setRuntimeSettings,
  upsertDictionary,
  upsertSnippet,
} from "@/lib/tauri";

type Tab = "live" | "history" | "stats" | "dictionary" | "snippets" | "settings";
type CloudMode = "local_only" | "hybrid" | "cloud_preferred";
type UpdateCheckOptions = {
  silent?: boolean;
  ignoreDeferrals?: boolean;
  source?: "manual" | "startup-auto";
};
type UpdateInstallOptions = {
  autoExit?: boolean;
  source?: "manual" | "banner" | "idle-auto";
};
type UpdateTelemetryEvent = {
  id: string;
  at: string;
  event: string;
  detail: string;
  source: "manual" | "startup-auto" | "idle-auto" | "system";
};

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
  { value: "distil-large-v3", label: "Distil Large v3 (recommended)" },
  { value: "large-v3-turbo", label: "Large v3 Turbo (fast + high quality)" },
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

const CLOUD_MODE_OPTIONS: Array<{ value: CloudMode; label: string; hint: string }> = [
  {
    value: "local_only",
    label: "Local only (default)",
    hint: "Never use cloud transcription.",
  },
  {
    value: "hybrid",
    label: "Hybrid fallback",
    hint: "Use cloud only when local confidence/quality gates fail.",
  },
  {
    value: "cloud_preferred",
    label: "Cloud preferred",
    hint: "Prioritize cloud when available, keep local as backup.",
  },
];

const MIC_CALIBRATION_STORAGE_KEY = "dictum-mic-calibration-v1";
const UPDATE_REPO_STORAGE_KEY = "dictum-update-repo-v1";
const DEFAULT_UPDATE_REPO = "latticelabs/dictum";
const UPDATE_AUTO_CHECK_STORAGE_KEY = "dictum-update-auto-check-v1";
const UPDATE_SKIP_VERSION_STORAGE_KEY = "dictum-update-skip-version-v1";
const UPDATE_REMIND_UNTIL_STORAGE_KEY = "dictum-update-remind-until-v1";
const UPDATE_LAST_CHECKED_STORAGE_KEY = "dictum-update-last-checked-v1";
const UPDATE_AUTO_INSTALL_IDLE_STORAGE_KEY = "dictum-update-auto-install-idle-v1";
const UPDATE_TELEMETRY_STORAGE_KEY = "dictum-update-telemetry-v1";
const UPDATE_IDLE_INSTALL_GRACE_MS = 120_000;

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

const normalizeWords = (text: string): string[] =>
  text.toLowerCase().match(/[a-z0-9']+/g) ?? [];

const jaccardSimilarity = (a: string, b: string): number => {
  const aw = normalizeWords(a);
  const bw = normalizeWords(b);
  if (aw.length === 0 || bw.length === 0) return 0;
  const aSet = new Set(aw);
  const bSet = new Set(bw);
  let intersection = 0;
  aSet.forEach((token) => {
    if (bSet.has(token)) intersection += 1;
  });
  const union = aSet.size + bSet.size - intersection;
  return union <= 0 ? 0 : intersection / union;
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
  const { defaultDevice, recommendedDevice, devices, loading } = useAudioDevices();

  const feedRef = useRef<HTMLDivElement>(null);
  const pushToTalkRef = useRef(false);
  const latestRmsRef = useRef(0);
  const manualDeviceSelectionRef = useRef(false);
  const manualModelSelectionRef = useRef(false);
  const autoUpdateCheckedRef = useRef(false);
  const lastUserInteractionAtRef = useRef(Date.now());
  const autoInstallAttemptedForVersionRef = useRef<string | null>(null);

  const [tab, setTab] = useState<Tab>("live");
  const [copyState, setCopyState] = useState<"idle" | "done" | "error">("idle");
  const [selectedDeviceName, setSelectedDeviceName] = useState<string | null>(null);
  const [modelProfile, setModelProfile] = useState("distil-large-v3");
  const [performanceProfile, setPerformanceProfile] = useState("whisper_balanced_english");
  const [toggleShortcut, setToggleShortcut] = useState("Ctrl+Shift+Space");
  const [ortEp, setOrtEp] = useState("auto");
  const [ortIntraThreads, setOrtIntraThreads] = useState(0);
  const [ortInterThreads, setOrtInterThreads] = useState(0);
  const [ortParallel, setOrtParallel] = useState(true);
  const [languageHint, setLanguageHint] = useState("english");
  const [pillVisualizerSensitivity, setPillVisualizerSensitivity] = useState(10);
  const [inputGainBoost, setInputGainBoost] = useState(1);
  const [postUtteranceRefine, setPostUtteranceRefine] = useState(false);
  const [phraseBiasTerms, setPhraseBiasTerms] = useState("");
  const [openAiApiKeyInput, setOpenAiApiKeyInput] = useState("");
  const [hasOpenAiApiKey, setHasOpenAiApiKey] = useState(false);
  const [cloudMode, setCloudMode] = useState<CloudMode>("local_only");
  const [cloudOptIn, setCloudOptIn] = useState(false);
  const [reliabilityMode, setReliabilityMode] = useState(true);
  const [onboardingCompleted, setOnboardingCompleted] = useState(false);
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [historyEnabled, setHistoryEnabled] = useState(true);
  const [retentionDays, setRetentionDays] = useState(90);
  const [runtimeMsg, setRuntimeMsg] = useState<string | null>(null);
  const [calibrationMsg, setCalibrationMsg] = useState<string | null>(null);
  const [isCalibrating, setIsCalibrating] = useState(false);
  const [isBenchmarkTuning, setIsBenchmarkTuning] = useState(false);
  const [updateRepoSlug, setUpdateRepoSlug] = useState(DEFAULT_UPDATE_REPO);
  const [currentAppVersion, setCurrentAppVersion] = useState<string>("dev");
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo | null>(null);
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
  const [isInstallingUpdate, setIsInstallingUpdate] = useState(false);
  const [updateAutoCheckEnabled, setUpdateAutoCheckEnabled] = useState(true);
  const [updateSkipVersion, setUpdateSkipVersion] = useState<string | null>(null);
  const [updateRemindUntilMs, setUpdateRemindUntilMs] = useState(0);
  const [updateLastCheckedAt, setUpdateLastCheckedAt] = useState<string | null>(null);
  const [updateAutoInstallWhenIdle, setUpdateAutoInstallWhenIdle] = useState(false);
  const [updateTelemetry, setUpdateTelemetry] = useState<UpdateTelemetryEvent[]>([]);
  const [updateLogCopied, setUpdateLogCopied] = useState<"idle" | "done" | "error">("idle");
  const [activeFixSegmentId, setActiveFixSegmentId] = useState<string | null>(null);
  const [activeFixText, setActiveFixText] = useState("");

  const [historyItems, setHistoryItems] = useState<HistoryItem[]>([]);
  const [historyQuery, setHistoryQuery] = useState("");
  const [stats, setStats] = useState<StatsPayload | null>(null);
  const [perfSnapshot, setPerfSnapshot] = useState<PerfSnapshot | null>(null);
  const [dictionary, setDictionary] = useState<DictionaryEntry[]>([]);
  const [snippets, setSnippets] = useState<SnippetEntry[]>([]);
  const [modelCatalog, setModelCatalog] = useState<ModelProfileMetadata[]>([]);
  const [modelRecommendation, setModelRecommendation] = useState<ModelProfileRecommendation | null>(null);
  const [learnedCorrections, setLearnedCorrections] = useState<LearnedCorrection[]>([]);
  const [correctionHeardInput, setCorrectionHeardInput] = useState("");
  const [correctionFixedInput, setCorrectionFixedInput] = useState("");
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

  const activeModelMeta = useMemo(
    () => modelCatalog.find((item) => item.profile === modelProfile) ?? null,
    [modelCatalog, modelProfile],
  );
  const recommendedModelMeta = useMemo(() => {
    if (!modelRecommendation) return null;
    return modelCatalog.find((item) => item.profile === modelRecommendation.recommendedProfile) ?? null;
  }, [modelCatalog, modelRecommendation]);
  const lowConfidenceFinals = useMemo(
    () =>
      segments
        .filter((seg) => seg.kind === "final" && (seg.confidence ?? 1) < 0.74)
        .slice(-6),
    [segments],
  );
  const correctionSuggestions = useMemo(() => {
    const suggestions: Array<{
      id: string;
      heard: string;
      corrected: string;
      confidence: number;
      score: number;
    }> = [];
    for (const seg of lowConfidenceFinals) {
      const segConf = seg.confidence ?? 0.6;
      for (const rule of learnedCorrections) {
        const sim = jaccardSimilarity(seg.text, rule.heard);
        const contains = seg.text.toLowerCase().includes(rule.heard.toLowerCase()) ? 1 : 0;
        const weighted = (1 - segConf) * (1 + Math.min(rule.hits, 12) / 12) * (sim + contains * 0.4);
        if (weighted < 0.18) continue;
        suggestions.push({
          id: `${seg.id}:${rule.heard}:${rule.corrected}`,
          heard: seg.text,
          corrected: rule.corrected,
          confidence: segConf,
          score: weighted,
        });
      }
    }
    suggestions.sort((a, b) => b.score - a.score);
    return suggestions.slice(0, 5);
  }, [learnedCorrections, lowConfidenceFinals]);

  const appendUpdateTelemetry = useCallback((
    event: string,
    detail: string,
    source: UpdateTelemetryEvent["source"] = "system",
  ) => {
    setUpdateTelemetry((prev) => {
      const entry: UpdateTelemetryEvent = {
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        at: new Date().toISOString(),
        event,
        detail,
        source,
      };
      return [entry, ...prev].slice(0, 80);
    });
  }, []);

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
    getAppVersion()
      .then((version) => setCurrentAppVersion(version))
      .catch(() => setCurrentAppVersion("dev"));
  }, []);

  useEffect(() => {
    try {
      const savedRepo = localStorage.getItem(UPDATE_REPO_STORAGE_KEY);
      if (savedRepo && savedRepo.trim()) {
        setUpdateRepoSlug(savedRepo.trim());
      }
      const savedAutoCheck = localStorage.getItem(UPDATE_AUTO_CHECK_STORAGE_KEY);
      if (savedAutoCheck === "0") {
        setUpdateAutoCheckEnabled(false);
      } else if (savedAutoCheck === "1") {
        setUpdateAutoCheckEnabled(true);
      }
      const savedSkipVersion = localStorage.getItem(UPDATE_SKIP_VERSION_STORAGE_KEY);
      if (savedSkipVersion && savedSkipVersion.trim()) {
        setUpdateSkipVersion(savedSkipVersion.trim());
      }
      const savedRemindUntil = Number(localStorage.getItem(UPDATE_REMIND_UNTIL_STORAGE_KEY) || "0");
      if (Number.isFinite(savedRemindUntil) && savedRemindUntil > 0) {
        setUpdateRemindUntilMs(savedRemindUntil);
      }
      const savedLastChecked = localStorage.getItem(UPDATE_LAST_CHECKED_STORAGE_KEY);
      if (savedLastChecked && savedLastChecked.trim()) {
        setUpdateLastCheckedAt(savedLastChecked.trim());
      }
      const savedAutoInstallIdle = localStorage.getItem(UPDATE_AUTO_INSTALL_IDLE_STORAGE_KEY);
      if (savedAutoInstallIdle === "1") {
        setUpdateAutoInstallWhenIdle(true);
      } else if (savedAutoInstallIdle === "0") {
        setUpdateAutoInstallWhenIdle(false);
      }
      const savedTelemetry = localStorage.getItem(UPDATE_TELEMETRY_STORAGE_KEY);
      if (savedTelemetry) {
        const parsed = JSON.parse(savedTelemetry) as UpdateTelemetryEvent[];
        if (Array.isArray(parsed)) {
          setUpdateTelemetry(parsed.slice(0, 80));
        }
      }
    } catch {
      // Ignore local storage failures.
    }
  }, []);

  useEffect(() => {
    try {
      localStorage.setItem(UPDATE_REPO_STORAGE_KEY, updateRepoSlug.trim() || DEFAULT_UPDATE_REPO);
    } catch {
      // Ignore local storage failures.
    }
  }, [updateRepoSlug]);

  useEffect(() => {
    try {
      localStorage.setItem(UPDATE_AUTO_CHECK_STORAGE_KEY, updateAutoCheckEnabled ? "1" : "0");
    } catch {
      // Ignore local storage failures.
    }
  }, [updateAutoCheckEnabled]);

  useEffect(() => {
    try {
      if (updateSkipVersion && updateSkipVersion.trim()) {
        localStorage.setItem(UPDATE_SKIP_VERSION_STORAGE_KEY, updateSkipVersion.trim());
      } else {
        localStorage.removeItem(UPDATE_SKIP_VERSION_STORAGE_KEY);
      }
    } catch {
      // Ignore local storage failures.
    }
  }, [updateSkipVersion]);

  useEffect(() => {
    try {
      if (updateRemindUntilMs > 0) {
        localStorage.setItem(UPDATE_REMIND_UNTIL_STORAGE_KEY, String(Math.floor(updateRemindUntilMs)));
      } else {
        localStorage.removeItem(UPDATE_REMIND_UNTIL_STORAGE_KEY);
      }
    } catch {
      // Ignore local storage failures.
    }
  }, [updateRemindUntilMs]);

  useEffect(() => {
    try {
      if (updateLastCheckedAt && updateLastCheckedAt.trim()) {
        localStorage.setItem(UPDATE_LAST_CHECKED_STORAGE_KEY, updateLastCheckedAt);
      } else {
        localStorage.removeItem(UPDATE_LAST_CHECKED_STORAGE_KEY);
      }
    } catch {
      // Ignore local storage failures.
    }
  }, [updateLastCheckedAt]);

  useEffect(() => {
    try {
      localStorage.setItem(UPDATE_AUTO_INSTALL_IDLE_STORAGE_KEY, updateAutoInstallWhenIdle ? "1" : "0");
    } catch {
      // Ignore local storage failures.
    }
  }, [updateAutoInstallWhenIdle]);

  useEffect(() => {
    try {
      localStorage.setItem(UPDATE_TELEMETRY_STORAGE_KEY, JSON.stringify(updateTelemetry.slice(0, 80)));
    } catch {
      // Ignore local storage failures.
    }
  }, [updateTelemetry]);

  useEffect(() => {
    Promise.all([getRuntimeSettings(), getPrivacySettings()])
      .then(([runtime, privacy]) => {
        setModelProfile(runtime.modelProfile || "distil-large-v3");
        setPerformanceProfile(runtime.performanceProfile || "whisper_balanced_english");
        setToggleShortcut(runtime.toggleShortcut || "Ctrl+Shift+Space");
        setOrtEp(runtime.ortEp || "auto");
        setOrtIntraThreads(runtime.ortIntraThreads ?? 0);
        setOrtInterThreads(runtime.ortInterThreads ?? 0);
        setOrtParallel(runtime.ortParallel ?? true);
        setLanguageHint(runtime.languageHint || "english");
        setPillVisualizerSensitivity(runtime.pillVisualizerSensitivity || 10);
        setActivitySensitivity(runtime.activitySensitivity || 4.2);
        setActivityNoiseGate(runtime.activityNoiseGate ?? 0.0015);
        setActivityClipThreshold(runtime.activityClipThreshold ?? 0.32);
        setInputGainBoost(runtime.inputGainBoost || 1);
        setPostUtteranceRefine(runtime.postUtteranceRefine ?? false);
        setPhraseBiasTerms((runtime.phraseBiasTerms || []).join("\n"));
        setHasOpenAiApiKey(runtime.hasOpenAiApiKey ?? false);
        const mode = (runtime.cloudMode ||
          (privacy.cloudOptIn ? "hybrid" : "local_only")) as CloudMode;
        setCloudMode(mode);
        setCloudOptIn(mode !== "local_only");
        setReliabilityMode(runtime.reliabilityMode ?? true);
        setOnboardingCompleted(runtime.onboardingCompleted ?? false);
        setShowOnboarding(!(runtime.onboardingCompleted ?? false));
        setHistoryEnabled(privacy.historyEnabled);
        setRetentionDays(privacy.retentionDays ?? 90);
      })
      .catch((err) => console.warn("Could not fetch runtime/privacy settings:", err));
  }, []);

  useEffect(() => {
    Promise.all([getModelProfileCatalog(), getModelProfileRecommendation()])
      .then(([catalog, recommendation]) => {
        setModelCatalog(catalog);
        setModelRecommendation(recommendation);
      })
      .catch((err) => console.warn("Could not fetch model profile metadata:", err));
  }, []);

  useEffect(() => {
    getLearnedCorrections()
      .then((rules) => setLearnedCorrections(rules))
      .catch((err) => console.warn("Could not fetch learned corrections:", err));
  }, []);

  useEffect(() => {
    setCloudOptIn(cloudMode !== "local_only");
  }, [cloudMode]);

  useEffect(() => {
    if (!showOnboarding || onboardingCompleted) return;
    if (!modelRecommendation || manualModelSelectionRef.current) return;
    if (modelRecommendation.recommendedProfile === modelProfile) return;
    if (modelProfile !== "distil-large-v3") return;

    setModelProfile(modelRecommendation.recommendedProfile);
    if (modelRecommendation.suggestedOrtEp === "cpu" || modelRecommendation.suggestedOrtEp === "directml") {
      setOrtEp(modelRecommendation.suggestedOrtEp);
    }
    setRuntimeMsg(`Auto-selected recommended model: ${modelRecommendation.recommendedProfile}.`);
  }, [modelProfile, modelRecommendation, onboardingCompleted, showOnboarding]);

  useEffect(() => {
    if (performanceProfile === "whisper_balanced_english" && languageHint !== "english") {
      setLanguageHint("english");
    }
  }, [performanceProfile, languageHint]);

  useEffect(() => {
    if (devices.length === 0) return;

    const selectedDevice =
      selectedDeviceName ? devices.find((d) => d.name === selectedDeviceName) ?? null : null;
    const preferredByHeuristic =
      recommendedDevice ?? defaultDevice ?? devices.find((d) => !d.isLoopbackLike) ?? devices[0] ?? null;

    if (!preferredByHeuristic) return;
    if (!selectedDeviceName || !selectedDevice) {
      setSelectedDeviceName(preferredByHeuristic.name);
      void setPreferredInputDevice(preferredByHeuristic.name).catch(console.error);
      return;
    }

    if (
      !manualDeviceSelectionRef.current &&
      selectedDevice.isLoopbackLike &&
      !preferredByHeuristic.isLoopbackLike &&
      preferredByHeuristic.name !== selectedDevice.name
    ) {
      setSelectedDeviceName(preferredByHeuristic.name);
      void setPreferredInputDevice(preferredByHeuristic.name).catch(console.error);
      setRuntimeMsg(
        `Switched input from '${selectedDevice.name}' to '${preferredByHeuristic.name}' to avoid system-audio capture.`,
      );
    }
  }, [defaultDevice, devices, recommendedDevice, selectedDeviceName]);

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

  useEffect(() => {
    if (updateLogCopied === "idle") return;
    const timer = window.setTimeout(() => setUpdateLogCopied("idle"), 1600);
    return () => window.clearTimeout(timer);
  }, [updateLogCopied]);

  useEffect(() => {
    const markInteraction = () => {
      lastUserInteractionAtRef.current = Date.now();
    };
    window.addEventListener("pointerdown", markInteraction, { passive: true });
    window.addEventListener("keydown", markInteraction);
    window.addEventListener("wheel", markInteraction, { passive: true });
    return () => {
      window.removeEventListener("pointerdown", markInteraction);
      window.removeEventListener("keydown", markInteraction);
      window.removeEventListener("wheel", markInteraction);
    };
  }, []);

  const handleModelProfileChange = useCallback((next: string) => {
    manualModelSelectionRef.current = true;
    setModelProfile(next);
  }, []);

  const applyRecommendedModel = useCallback(() => {
    if (!modelRecommendation) return;
    manualModelSelectionRef.current = true;
    setModelProfile(modelRecommendation.recommendedProfile);
    if (modelRecommendation.suggestedOrtEp === "cpu" || modelRecommendation.suggestedOrtEp === "directml") {
      setOrtEp(modelRecommendation.suggestedOrtEp);
    }
    setRuntimeMsg(`Applied recommended model: ${modelRecommendation.recommendedProfile}.`);
  }, [modelRecommendation]);

  const handleAutoTune = useCallback(async () => {
    try {
      const tuned = await runAutoTune();
      const runtime = tuned.runtimeSettings;
      setModelProfile(runtime.modelProfile || "distil-large-v3");
      setPerformanceProfile(runtime.performanceProfile || "whisper_balanced_english");
      setToggleShortcut(runtime.toggleShortcut || "Ctrl+Shift+Space");
      setOrtEp(runtime.ortEp || "auto");
      setOrtIntraThreads(runtime.ortIntraThreads ?? 0);
      setOrtInterThreads(runtime.ortInterThreads ?? 0);
      setOrtParallel(runtime.ortParallel ?? true);
      setLanguageHint(runtime.languageHint || "english");
      setPillVisualizerSensitivity(runtime.pillVisualizerSensitivity || 10);
      setActivitySensitivity(runtime.activitySensitivity || 4.2);
      setActivityNoiseGate(runtime.activityNoiseGate ?? 0.0015);
      setActivityClipThreshold(runtime.activityClipThreshold ?? 0.32);
      setInputGainBoost(runtime.inputGainBoost || 1);
      setPostUtteranceRefine(runtime.postUtteranceRefine ?? false);
      setPhraseBiasTerms((runtime.phraseBiasTerms || []).join("\n"));
      setCloudMode((runtime.cloudMode || "local_only") as CloudMode);
      setCloudOptIn(runtime.cloudOptIn);
      setReliabilityMode(runtime.reliabilityMode ?? true);
      setRuntimeMsg(tuned.summary);
      const refreshedRecommendation = await getModelProfileRecommendation();
      setModelRecommendation(refreshedRecommendation);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Auto tune failed: ${msg}`);
    }
  }, []);

  const handleCheckForUpdates = useCallback(async (options?: UpdateCheckOptions) => {
    if (isCheckingUpdate) return;
    const silent = options?.silent ?? false;
    const ignoreDeferrals = options?.ignoreDeferrals ?? false;
    const source = options?.source ?? "manual";
    setIsCheckingUpdate(true);
    appendUpdateTelemetry(
      "check.started",
      `Checking ${updateRepoSlug.trim() || DEFAULT_UPDATE_REPO}`,
      source === "startup-auto" ? "startup-auto" : "manual",
    );
    try {
      const info = await checkForAppUpdate(updateRepoSlug.trim() || DEFAULT_UPDATE_REPO);
      setUpdateInfo(info);
      setUpdateLastCheckedAt(new Date().toISOString());
      const now = Date.now();
      const skipped = !!updateSkipVersion && updateSkipVersion === info.latestVersion;
      const snoozed = updateRemindUntilMs > now;
      appendUpdateTelemetry(
        "check.completed",
        info.hasUpdate
          ? `Update ${info.currentVersion} -> ${info.latestVersion}${info.assetDownloadUrl ? " (installer found)" : " (no installer asset)"}${info.expectedInstallerSha256 ? " + checksum" : " + no checksum"}.`
          : `No update. Current ${info.currentVersion}.`,
        source === "startup-auto" ? "startup-auto" : "manual",
      );
      if (info.hasUpdate) {
        if (!ignoreDeferrals && skipped) {
          if (!silent) {
            setRuntimeMsg(`Version ${info.latestVersion} is currently skipped.`);
          }
        } else if (!ignoreDeferrals && snoozed) {
          if (!silent) {
            setRuntimeMsg(`Update ${info.latestVersion} is snoozed until ${new Date(updateRemindUntilMs).toLocaleString()}.`);
          }
        } else if (!silent) {
          setRuntimeMsg(`Update available: ${info.currentVersion} -> ${info.latestVersion}.`);
        }
      } else if (!silent) {
        setRuntimeMsg(`Dictum is up to date (${info.currentVersion}).`);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      appendUpdateTelemetry(
        "check.failed",
        msg,
        source === "startup-auto" ? "startup-auto" : "manual",
      );
      if (!silent) {
        setRuntimeMsg(`Update check failed: ${msg}`);
      }
    } finally {
      setIsCheckingUpdate(false);
    }
  }, [appendUpdateTelemetry, isCheckingUpdate, updateRemindUntilMs, updateRepoSlug, updateSkipVersion]);

  const handleInstallUpdate = useCallback(async (options?: UpdateInstallOptions) => {
    if (isInstallingUpdate) return;
    const assetUrl = updateInfo?.assetDownloadUrl;
    const expectedSha256 = updateInfo?.expectedInstallerSha256;
    if (!assetUrl) {
      setRuntimeMsg("No installer asset found for this release.");
      appendUpdateTelemetry("install.skipped", "No installer asset in selected release.", "system");
      return;
    }
    if (!expectedSha256) {
      setRuntimeMsg("No trusted checksum found for installer. Installation blocked.");
      appendUpdateTelemetry("install.skipped", "Missing expected installer checksum.", "system");
      return;
    }
    const source = options?.source ?? "manual";
    const autoExit = options?.autoExit ?? false;
    setIsInstallingUpdate(true);
    appendUpdateTelemetry(
      "install.started",
      `Launching installer for ${updateInfo?.latestVersion ?? "unknown version"}. autoExit=${autoExit}`,
      source === "idle-auto" ? "idle-auto" : source === "banner" ? "manual" : "manual",
    );
    try {
      const result = await downloadAndInstallAppUpdate(
        assetUrl,
        updateInfo?.assetName ?? null,
        true,
        autoExit,
        expectedSha256,
      );
      appendUpdateTelemetry(
        "install.launched",
        `${result} autoExit=${autoExit}`,
        source === "idle-auto" ? "idle-auto" : source === "banner" ? "manual" : "manual",
      );
      setRuntimeMsg(autoExit
        ? `${result} Dictum will close to complete installation.`
        : `${result} Close Dictum to finish update if prompted.`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      appendUpdateTelemetry(
        "install.failed",
        msg,
        source === "idle-auto" ? "idle-auto" : source === "banner" ? "manual" : "manual",
      );
      setRuntimeMsg(`Update install failed: ${msg}`);
    } finally {
      setIsInstallingUpdate(false);
    }
  }, [appendUpdateTelemetry, isInstallingUpdate, updateInfo]);

  const handleRemindLater = useCallback(() => {
    const remindUntil = Date.now() + (24 * 60 * 60 * 1000);
    setUpdateRemindUntilMs(remindUntil);
    appendUpdateTelemetry(
      "defer.remind_later",
      `Snoozed until ${new Date(remindUntil).toISOString()}.`,
      "manual",
    );
    setRuntimeMsg(`Update reminder snoozed until ${new Date(remindUntil).toLocaleString()}.`);
  }, [appendUpdateTelemetry]);

  const handleSkipUpdateVersion = useCallback(() => {
    if (!updateInfo?.latestVersion) return;
    setUpdateSkipVersion(updateInfo.latestVersion);
    appendUpdateTelemetry("defer.skip_version", `Skipped ${updateInfo.latestVersion}.`, "manual");
    setRuntimeMsg(`Skipped update ${updateInfo.latestVersion}.`);
  }, [appendUpdateTelemetry, updateInfo]);

  const clearUpdateDeferrals = useCallback(() => {
    setUpdateSkipVersion(null);
    setUpdateRemindUntilMs(0);
    appendUpdateTelemetry("defer.cleared", "Cleared skip/snooze preferences.", "manual");
    setRuntimeMsg("Cleared update skip/reminder preferences.");
  }, [appendUpdateTelemetry]);

  const handleExportUpdateTelemetry = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(JSON.stringify(updateTelemetry, null, 2));
      setUpdateLogCopied("done");
      setRuntimeMsg("Copied updater telemetry log to clipboard.");
    } catch {
      setUpdateLogCopied("error");
      setRuntimeMsg("Failed to copy updater telemetry log.");
    }
  }, [updateTelemetry]);

  useEffect(() => {
    if (!updateAutoCheckEnabled || autoUpdateCheckedRef.current) return;
    if (showOnboarding) return;
    autoUpdateCheckedRef.current = true;
    const timer = window.setTimeout(() => {
      void handleCheckForUpdates({ silent: true, ignoreDeferrals: false, source: "startup-auto" });
    }, 9000);
    return () => window.clearTimeout(timer);
  }, [handleCheckForUpdates, showOnboarding, updateAutoCheckEnabled]);

  useEffect(() => {
    if (!updateAutoInstallWhenIdle) return;
    if (showOnboarding) return;
    if (!updateInfo?.hasUpdate || !updateInfo.assetDownloadUrl || !updateInfo.expectedInstallerSha256) return;
    if (isListening || isInstallingUpdate || isCheckingUpdate) return;
    if (updateSkipVersion && updateSkipVersion === updateInfo.latestVersion) return;
    if (updateRemindUntilMs > Date.now()) return;

    const checkIdleAndInstall = () => {
      const latestVersion = updateInfo.latestVersion;
      if (!latestVersion) return;
      if (autoInstallAttemptedForVersionRef.current === latestVersion) return;
      if (isListening || isInstallingUpdate || isCheckingUpdate) return;
      const idleForMs = Date.now() - lastUserInteractionAtRef.current;
      if (idleForMs < UPDATE_IDLE_INSTALL_GRACE_MS) return;
      autoInstallAttemptedForVersionRef.current = latestVersion;
      appendUpdateTelemetry(
        "install.auto_idle_triggered",
        `Idle for ${Math.round(idleForMs / 1000)}s. Launching silent install for ${latestVersion}.`,
        "idle-auto",
      );
      void handleInstallUpdate({ source: "idle-auto", autoExit: true });
    };

    const timer = window.setInterval(checkIdleAndInstall, 12_000);
    checkIdleAndInstall();
    return () => window.clearInterval(timer);
  }, [
    appendUpdateTelemetry,
    handleInstallUpdate,
    isCheckingUpdate,
    isInstallingUpdate,
    isListening,
    showOnboarding,
    updateAutoInstallWhenIdle,
    updateInfo,
    updateRemindUntilMs,
    updateSkipVersion,
  ]);

  useEffect(() => {
    if (!updateInfo?.hasUpdate) {
      autoInstallAttemptedForVersionRef.current = null;
      return;
    }
    if (autoInstallAttemptedForVersionRef.current && autoInstallAttemptedForVersionRef.current !== updateInfo.latestVersion) {
      autoInstallAttemptedForVersionRef.current = null;
    }
  }, [updateInfo]);

  const handleLearnCorrection = useCallback(async () => {
    const heard = correctionHeardInput.trim();
    const corrected = correctionFixedInput.trim();
    if (!heard || !corrected) {
      setRuntimeMsg("Enter both heard and corrected text.");
      return;
    }
    try {
      const rules = await learnCorrection(heard, corrected);
      setLearnedCorrections(rules);
      setCorrectionHeardInput("");
      setCorrectionFixedInput("");
      setRuntimeMsg(`Learned correction: "${heard}" -> "${corrected}".`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to learn correction: ${msg}`);
    }
  }, [correctionFixedInput, correctionHeardInput]);

  const handleDeleteCorrection = useCallback(async (heard: string, corrected: string) => {
    try {
      const rules = await deleteLearnedCorrection(heard, corrected);
      setLearnedCorrections(rules);
      setRuntimeMsg(`Removed correction for "${heard}".`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to remove correction: ${msg}`);
    }
  }, []);

  const startInlineFix = useCallback((segmentId: string, heardText: string) => {
    setActiveFixSegmentId(segmentId);
    setActiveFixText(heardText.trim());
  }, []);

  const handleSaveInlineFix = useCallback(async (heardText: string) => {
    const heard = heardText.trim();
    const corrected = activeFixText.trim();
    if (!heard || !corrected) {
      setRuntimeMsg("Enter corrected text before saving.");
      return;
    }
    try {
      const rules = await learnCorrection(heard, corrected);
      setLearnedCorrections(rules);
      setCorrectionHeardInput("");
      setCorrectionFixedInput("");
      setActiveFixSegmentId(null);
      setActiveFixText("");
      setRuntimeMsg(`Saved correction: "${heard}" -> "${corrected}".`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to save correction: ${msg}`);
    }
  }, [activeFixText]);

  const applySuggestion = useCallback(async (heard: string, corrected: string) => {
    try {
      const rules = await learnCorrection(heard, corrected);
      setLearnedCorrections(rules);
      setRuntimeMsg(`Applied suggestion: "${heard}" -> "${corrected}".`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to apply suggestion: ${msg}`);
    }
  }, []);

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
      ortIntraThreads: number;
      ortInterThreads: number;
      ortParallel: boolean;
      languageHint: string;
      pillVisualizerSensitivity: number;
      activitySensitivity: number;
      activityNoiseGate: number;
      activityClipThreshold: number;
      inputGainBoost: number;
      postUtteranceRefine: boolean;
      phraseBiasTerms: string[];
      openAiApiKey: string | null;
      cloudMode: CloudMode;
      cloudOptIn: boolean;
      reliabilityMode: boolean;
      onboardingCompleted: boolean;
      historyEnabled: boolean;
      retentionDays: number;
    }>): Promise<boolean> => {
      const next = {
        modelProfile,
        performanceProfile,
        toggleShortcut,
        ortEp,
        ortIntraThreads,
        ortInterThreads,
        ortParallel,
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
        cloudMode,
        cloudOptIn: cloudMode !== "local_only",
        reliabilityMode,
        onboardingCompleted,
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
          next.ortIntraThreads,
          next.ortInterThreads,
          next.ortParallel,
          next.languageHint,
          next.pillVisualizerSensitivity,
          next.activitySensitivity,
          next.activityNoiseGate,
          next.activityClipThreshold,
          next.inputGainBoost,
          next.postUtteranceRefine,
          next.phraseBiasTerms,
          next.openAiApiKey,
          next.cloudMode,
          next.cloudOptIn,
          next.reliabilityMode,
          next.onboardingCompleted,
          next.historyEnabled,
          next.retentionDays,
        );
        setModelProfile(updated.modelProfile || "distil-large-v3");
        setPerformanceProfile(updated.performanceProfile || "whisper_balanced_english");
        setToggleShortcut(updated.toggleShortcut || "Ctrl+Shift+Space");
        setOrtEp(updated.ortEp || "auto");
        setOrtIntraThreads(updated.ortIntraThreads ?? 0);
        setOrtInterThreads(updated.ortInterThreads ?? 0);
        setOrtParallel(updated.ortParallel ?? true);
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
        setCloudMode((updated.cloudMode || "local_only") as CloudMode);
        setCloudOptIn(updated.cloudOptIn);
        setReliabilityMode(updated.reliabilityMode ?? true);
        setOnboardingCompleted(updated.onboardingCompleted ?? false);
        setHistoryEnabled(updated.historyEnabled);
        setRetentionDays(updated.retentionDays);
        if (updated.cloudMode !== "local_only" && !updated.hasOpenAiApiKey) {
          setRuntimeMsg("Saved. Add an OpenAI API key to use cloud mode.");
        } else {
          setRuntimeMsg("Saved.");
        }
        return true;
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setRuntimeMsg(`Failed to save settings: ${msg}`);
        return false;
      }
    },
    [
      modelProfile,
      performanceProfile,
      toggleShortcut,
      ortEp,
      ortIntraThreads,
      ortInterThreads,
      ortParallel,
      languageHint,
      pillVisualizerSensitivity,
      activitySensitivity,
      activityNoiseGate,
      activityClipThreshold,
      inputGainBoost,
      postUtteranceRefine,
      phraseBiasTerms,
      openAiApiKeyInput,
      cloudMode,
      reliabilityMode,
      onboardingCompleted,
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

  const completeOnboarding = useCallback(async () => {
    const ok = await applyRuntime({ onboardingCompleted: true });
    if (!ok) {
      return;
    }
    setShowOnboarding(false);
    setRuntimeMsg("Onboarding complete.");
  }, [applyRuntime]);

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
      setCalibrationMsg("Normal speech phase: speak at your usual volume...");
      await new Promise((r) => window.setTimeout(r, 250));
      const normal = await collectRmsSamples(2200);

      const ambientP90 = percentile(ambient, 0.9);
      const whisperP70 = percentile(whisper, 0.7);
      const normalP80 = percentile(normal, 0.8);
      const recommendedNoiseGate = clamp(ambientP90 * 1.45, 0.0004, 0.03);
      const recommendedActivitySensitivity = clamp(
        0.34 / Math.max(0.0001, whisperP70 - recommendedNoiseGate),
        1.0,
        20.0,
      );
      const recommendedPillSensitivity = clamp(recommendedActivitySensitivity * 1.12, 1.0, 20.0);
      const recommendedInputGainBoost = clamp(0.02 / Math.max(0.0001, whisperP70), 0.5, 8.0);
      const recommendedClipThreshold = clamp(
        Math.max(normalP80 * 3.2, ambientP90 * 12, whisperP70 * 8),
        0.12,
        0.95,
      );

      setActivityNoiseGate(recommendedNoiseGate);
      setActivitySensitivity(recommendedActivitySensitivity);
      setActivityClipThreshold(recommendedClipThreshold);
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
            activityClipThreshold: recommendedClipThreshold,
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
        activityClipThreshold: recommendedClipThreshold,
        inputGainBoost: recommendedInputGainBoost,
      });
      const envLabel = ambientP90 > 0.01 ? "noisy" : "stable";
      setCalibrationMsg(
        `Diagnostics complete (${envLabel} room). Sensitivity + gain tuned for whispers and normal speech.`,
      );
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

  const runBenchmarkTune = useCallback(async () => {
    if (isBenchmarkTuning) return;
    setIsBenchmarkTuning(true);
    setCalibrationMsg("Benchmark ambient phase: stay silent...");
    const startedByBenchmark = !isListening;
    try {
      if (!isListening) {
        await startEngine(selectedDeviceName);
        await new Promise((r) => window.setTimeout(r, 500));
      }

      const ambient = await collectRmsSamples(2300);
      setCalibrationMsg("Benchmark whisper phase: whisper two short phrases...");
      await new Promise((r) => window.setTimeout(r, 280));
      const whisper = await collectRmsSamples(2700);
      setCalibrationMsg("Benchmark normal phase: speak naturally...");
      await new Promise((r) => window.setTimeout(r, 280));
      const normal = await collectRmsSamples(2500);
      setCalibrationMsg("Collecting latency/fallback telemetry...");
      await new Promise((r) => window.setTimeout(r, 900));
      const perf = await getPerfSnapshot();

      const ambientP90 = percentile(ambient, 0.9);
      const whisperP70 = percentile(whisper, 0.7);
      const normalP80 = percentile(normal, 0.8);
      const finalizeP95 = perf.finalizeMs?.p95Ms ?? 0;
      const fallbackRate = perf.diagnostics.finalSegmentsSeen
        ? (perf.diagnostics.fallbackStubTyped / perf.diagnostics.finalSegmentsSeen) * 100
        : 0;

      const tuned = await runBenchmarkAutoTune(
        ambientP90,
        whisperP70,
        normalP80,
        finalizeP95,
        fallbackRate,
      );
      const runtime = tuned.runtimeSettings;
      setModelProfile(runtime.modelProfile || "distil-large-v3");
      setPerformanceProfile(runtime.performanceProfile || "whisper_balanced_english");
      setToggleShortcut(runtime.toggleShortcut || "Ctrl+Shift+Space");
      setOrtEp(runtime.ortEp || "auto");
      setOrtIntraThreads(runtime.ortIntraThreads ?? 0);
      setOrtInterThreads(runtime.ortInterThreads ?? 0);
      setOrtParallel(runtime.ortParallel ?? true);
      setLanguageHint(runtime.languageHint || "english");
      setPillVisualizerSensitivity(runtime.pillVisualizerSensitivity || 10);
      setActivitySensitivity(runtime.activitySensitivity || 4.2);
      setActivityNoiseGate(runtime.activityNoiseGate ?? 0.0015);
      setActivityClipThreshold(runtime.activityClipThreshold ?? 0.32);
      setInputGainBoost(runtime.inputGainBoost || 1);
      setRuntimeMsg(tuned.summary);
      setCalibrationMsg(
        `Benchmark tune complete. p95 ${Math.round(
          tuned.measuredFinalizeP95Ms,
        )}ms · fallback ${tuned.measuredFallbackRatePct.toFixed(1)}%.`,
      );
      const refreshedRecommendation = await getModelProfileRecommendation();
      setModelRecommendation(refreshedRecommendation);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setCalibrationMsg(`Benchmark tune failed: ${msg}`);
    } finally {
      if (startedByBenchmark) {
        await stopEngine().catch(() => undefined);
      }
      setIsBenchmarkTuning(false);
    }
  }, [
    collectRmsSamples,
    getPerfSnapshot,
    isBenchmarkTuning,
    isListening,
    selectedDeviceName,
    startEngine,
    stopEngine,
  ]);

  const refreshHistory = useCallback(async () => {
    const page = await getHistory(1, 100, historyQuery || null);
    setHistoryItems(page.items);
  }, [historyQuery]);

  const refreshStats = useCallback(async () => {
    const [statsData, perfData] = await Promise.all([getStats(30), getPerfSnapshot()]);
    setStats(statsData);
    setPerfSnapshot(perfData);
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
  const fallbackStubRate = perfSnapshot?.diagnostics.finalSegmentsSeen
    ? (perfSnapshot.diagnostics.fallbackStubTyped / perfSnapshot.diagnostics.finalSegmentsSeen) * 100
    : 0;
  const finalizeP95 = perfSnapshot?.finalizeMs.p95Ms ?? 0;
  const nowMs = Date.now();
  const updateIsSkipped = !!(updateInfo?.hasUpdate && updateSkipVersion && updateInfo.latestVersion === updateSkipVersion);
  const updateIsSnoozed = !!(updateInfo?.hasUpdate && updateRemindUntilMs > nowMs);
  const showUpdateBanner = !!(updateInfo?.hasUpdate && !updateIsSkipped && !updateIsSnoozed);
  const updatePublishedLabel = updateInfo?.publishedAt
    ? new Date(updateInfo.publishedAt).toLocaleDateString()
    : null;

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

      {showUpdateBanner && (
        <section className="update-banner" data-no-drag>
          <div className="update-banner-copy">
            <span className="update-banner-kicker">Update Ready</span>
            <strong>
              Dictum {updateInfo.currentVersion} {"->"} {updateInfo.latestVersion}
            </strong>
            <small>
              {updateInfo.releaseName ?? "New release available"}
              {updatePublishedLabel ? ` · ${updatePublishedLabel}` : ""}
            </small>
          </div>
          <div className="update-banner-actions">
            <button
              className="action-btn"
              onClick={() => void handleInstallUpdate({ source: "banner", autoExit: false })}
              disabled={isInstallingUpdate || !updateInfo.assetDownloadUrl || !updateInfo.expectedInstallerSha256}
            >
              {isInstallingUpdate ? "Launching..." : "Install"}
            </button>
            <button className="action-btn" onClick={handleRemindLater}>
              Remind Later
            </button>
            <button className="action-btn" onClick={handleSkipUpdateVersion}>
              Skip Version
            </button>
          </div>
        </section>
      )}

      {tab === "live" && (
        <>
          <div className="transcript-scroll selectable" ref={feedRef}>
            {hasSegments ? (
              <div className="transcript-feed">
                {segments.map((seg) => (
                  <div key={seg.id} className={`seg-row ${seg.kind === "partial" ? "is-partial" : "is-final"}`}>
                    <p className={seg.kind === "partial" ? "seg-partial" : "seg-final"}>
                      {seg.text}
                      {seg.kind === "final" && typeof seg.confidence === "number" && (
                        <span className="seg-confidence">
                          {(seg.confidence * 100).toFixed(0)}%
                        </span>
                      )}
                    </p>
                    {seg.kind === "final" && (
                      <div className="seg-actions">
                        <button
                          type="button"
                          className="seg-fix-btn"
                          onClick={() => startInlineFix(seg.id, seg.text)}
                        >
                          Fix this
                        </button>
                      </div>
                    )}
                    {activeFixSegmentId === seg.id && (
                      <div className="seg-fix-form">
                        <input
                          className="runtime-select panel-input"
                          value={activeFixText}
                          onChange={(e) => setActiveFixText(e.target.value)}
                          placeholder="Corrected text"
                        />
                        <button
                          type="button"
                          className="action-btn"
                          onClick={() => void handleSaveInlineFix(seg.text)}
                        >
                          Save
                        </button>
                        <button
                          type="button"
                          className="action-btn"
                          onClick={() => {
                            setActiveFixSegmentId(null);
                            setActiveFixText("");
                          }}
                        >
                          Cancel
                        </button>
                      </div>
                    )}
                  </div>
                ))}
                {correctionSuggestions.length > 0 && (
                  <div className="suggestion-strip">
                    <span className="suggestion-label">Likely corrections</span>
                    {correctionSuggestions.map((s) => (
                      <button
                        key={s.id}
                        type="button"
                        className="suggestion-chip"
                        onClick={() => void applySuggestion(s.heard, s.corrected)}
                        title={`confidence ${(s.confidence * 100).toFixed(0)}%`}
                      >
                        {s.corrected}
                      </button>
                    ))}
                  </div>
                )}
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
                  manualDeviceSelectionRef.current = true;
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
                    {d.name}
                    {d.isRecommended ? " (recommended)" : d.isDefault ? " *" : ""}
                    {d.isLoopbackLike ? " (system output)" : ""}
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
                  <div className="stat-card">
                    <b>{`${Math.round(fallbackStubRate)}%`}</b>
                    <span>Fallback Stub Rate</span>
                  </div>
                  <div className="stat-card">
                    <b>{perfSnapshot?.diagnostics.shortcutToggleDropped ?? 0}</b>
                    <span>Shortcut Drops</span>
                  </div>
                  <div className="stat-card">
                    <b>{Math.round(finalizeP95)} ms</b>
                    <span>Finalize p95</span>
                  </div>
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
                  <button className="action-btn" onClick={() => setShowOnboarding(true)}>
                    Run Onboarding
                  </button>
                  <button
                    className="action-btn"
                    onClick={() => void handleCheckForUpdates({ silent: false, ignoreDeferrals: true })}
                    disabled={isCheckingUpdate}
                  >
                    {isCheckingUpdate ? "Checking..." : "Check Updates"}
                  </button>
                  <button className="action-btn" onClick={() => void handleAutoTune()}>
                    Auto Tune
                  </button>
                  <button className="action-btn" onClick={() => void runBenchmarkTune()} disabled={isBenchmarkTuning}>
                    {isBenchmarkTuning ? "Benchmarking..." : "Benchmark Tune"}
                  </button>
                  <button className="action-btn" onClick={() => void runMicCalibration()} disabled={isCalibrating}>
                    {isCalibrating ? "Running..." : "Diagnostics Wizard"}
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
                      <select className="settings-input" value={modelProfile} onChange={(e) => handleModelProfileChange(e.target.value)}>
                        {MODEL_PROFILE_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
                      </select>
                    </label>
                    {activeModelMeta && (
                      <p className="settings-note">
                        Speed: {activeModelMeta.speedTier} · Quality: {activeModelMeta.qualityTier} · Min RAM: {activeModelMeta.minRamGb} GB
                        {activeModelMeta.minVramGb ? ` · Min VRAM: ${activeModelMeta.minVramGb} GB` : ""}
                        {activeModelMeta.englishOptimized ? " · English-optimized" : ""}
                      </p>
                    )}
                    {modelRecommendation && (
                      <div className="settings-inline-actions">
                        <button className="action-btn" onClick={applyRecommendedModel}>
                          Use Recommended ({modelRecommendation.recommendedProfile})
                        </button>
                        <span className="settings-note">{modelRecommendation.reason}</span>
                      </div>
                    )}
                    <label className="settings-field">
                      <span>Runtime</span>
                      <select className="settings-input" value={ortEp} onChange={(e) => setOrtEp(e.target.value)}>
                        {ORT_EP_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
                      </select>
                    </label>
                    <p className="settings-note">
                      ONNX threads: intra {ortIntraThreads || "auto"} · inter {ortInterThreads || "auto"} · parallel {ortParallel ? "on" : "off"}
                    </p>
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
                  <label className="settings-switch">
                    <input type="checkbox" checked={reliabilityMode} onChange={(e) => setReliabilityMode(e.target.checked)} />
                    <span>Whisper reliability mode (re-try low-confidence finals)</span>
                  </label>
                </article>

                <article className="settings-card">
                  <div className="settings-card-header">
                    <h3>Live Corrections</h3>
                    <p>Teach Dictum how to fix common mishears on future finals.</p>
                  </div>
                  <div className="settings-fields">
                    <label className="settings-field">
                      <span>Heard</span>
                      <input
                        className="settings-input"
                        value={correctionHeardInput}
                        onChange={(e) => setCorrectionHeardInput(e.target.value)}
                        placeholder="ex: ladder labs"
                      />
                    </label>
                    <label className="settings-field">
                      <span>Corrected</span>
                      <input
                        className="settings-input"
                        value={correctionFixedInput}
                        onChange={(e) => setCorrectionFixedInput(e.target.value)}
                        placeholder="ex: Lattice Labs"
                      />
                    </label>
                  </div>
                  <div className="settings-inline-actions">
                    <button className="action-btn" onClick={() => void handleLearnCorrection()}>
                      Learn Correction
                    </button>
                    <span className="settings-note">{learnedCorrections.length} learned rules</span>
                  </div>
                  <div className="panel-list">
                    {learnedCorrections.slice(0, 8).map((rule) => (
                      <article key={`${rule.heard}:${rule.corrected}`} className="panel-card">
                        <div className="panel-meta">
                          <span>{rule.heard}</span>
                          <span>→</span>
                          <span>{rule.corrected}</span>
                          <span>hits {rule.hits}</span>
                        </div>
                        <button
                          className="action-btn"
                          onClick={() => void handleDeleteCorrection(rule.heard, rule.corrected)}
                        >
                          Remove
                        </button>
                      </article>
                    ))}
                    {learnedCorrections.length === 0 && (
                      <p className="settings-note">No learned corrections yet.</p>
                    )}
                  </div>
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
                  <label className="settings-field">
                    <span>Cloud Mode</span>
                    <select
                      className="settings-input"
                      value={cloudMode}
                      onChange={(e) => setCloudMode(e.target.value as CloudMode)}
                    >
                      {CLOUD_MODE_OPTIONS.map((opt) => (
                        <option key={opt.value} value={opt.value}>
                          {opt.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  <p className="settings-note">
                    {CLOUD_MODE_OPTIONS.find((opt) => opt.value === cloudMode)?.hint}
                  </p>
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

                <article className="settings-card">
                  <div className="settings-card-header">
                    <h3>App Updates</h3>
                    <p>Background startup checks, manual check, and installer launch from GitHub Releases.</p>
                  </div>
                  <label className="settings-switch">
                    <input
                      type="checkbox"
                      checked={updateAutoCheckEnabled}
                      onChange={(e) => setUpdateAutoCheckEnabled(e.target.checked)}
                    />
                    <span>Auto-check for updates on startup</span>
                  </label>
                  <label className="settings-switch">
                    <input
                      type="checkbox"
                      checked={updateAutoInstallWhenIdle}
                      onChange={(e) => setUpdateAutoInstallWhenIdle(e.target.checked)}
                    />
                    <span>Auto-install updates when idle (2+ min, not listening)</span>
                  </label>
                  <label className="settings-field">
                    <span>Repository</span>
                    <input
                      className="settings-input"
                      value={updateRepoSlug}
                      onChange={(e) => setUpdateRepoSlug(e.target.value)}
                      placeholder="owner/repo"
                    />
                  </label>
                  <div className="settings-inline-actions">
                    <button
                      className="action-btn"
                      onClick={() => void handleCheckForUpdates({ silent: false, ignoreDeferrals: true })}
                      disabled={isCheckingUpdate}
                    >
                      {isCheckingUpdate ? "Checking..." : "Check for updates"}
                    </button>
                    <button
                      className="action-btn"
                      onClick={() => void handleInstallUpdate({ source: "manual", autoExit: false })}
                      disabled={isInstallingUpdate || !updateInfo?.hasUpdate || !updateInfo?.assetDownloadUrl || !updateInfo?.expectedInstallerSha256}
                    >
                      {isInstallingUpdate ? "Launching..." : "Install available update"}
                    </button>
                    <button className="action-btn" onClick={handleRemindLater} disabled={!updateInfo?.hasUpdate}>
                      Remind Later
                    </button>
                    <button className="action-btn" onClick={handleSkipUpdateVersion} disabled={!updateInfo?.hasUpdate}>
                      Skip This Version
                    </button>
                    <button className="action-btn" onClick={clearUpdateDeferrals}>
                      Clear Skip/Snooze
                    </button>
                    {updateInfo?.htmlUrl && (
                      <a className="action-btn" href={updateInfo.htmlUrl} target="_blank" rel="noreferrer">
                        Open release page
                      </a>
                    )}
                    <button className="action-btn" onClick={() => void handleExportUpdateTelemetry()}>
                      {updateLogCopied === "done"
                        ? "Log Copied"
                        : updateLogCopied === "error"
                          ? "Copy Failed"
                          : "Copy Update Log"}
                    </button>
                  </div>
                  <p className="settings-note">
                    Last checked: {updateLastCheckedAt ? new Date(updateLastCheckedAt).toLocaleString() : "never"}.
                  </p>
                  <p className="settings-note">
                    {updateSkipVersion ? `Skipped version: ${updateSkipVersion}. ` : ""}
                    {updateRemindUntilMs > Date.now()
                      ? `Snoozed until ${new Date(updateRemindUntilMs).toLocaleString()}.`
                      : "No active snooze."}
                  </p>
                  <p className="settings-note">
                    Current version: {currentAppVersion}.
                    {updateInfo
                      ? updateInfo.hasUpdate
                        ? ` Latest: ${updateInfo.latestVersion}.`
                        : " Already up to date."
                      : " Check to fetch latest release metadata."}
                  </p>
                  <p className="settings-note">
                    {updateInfo
                      ? updateInfo.expectedInstallerSha256
                        ? `Installer hash verified from ${updateInfo.checksumAssetName ?? "SHA256SUMS.txt"} before launch.`
                        : "Installer verification data unavailable: update install is blocked."
                      : "No update metadata loaded yet."}
                  </p>
                  {updateInfo?.releaseNotes && (
                    <p className="settings-note">
                      {updateInfo.releaseNotes.slice(0, 260)}
                      {updateInfo.releaseNotes.length > 260 ? "..." : ""}
                    </p>
                  )}
                  <div className="panel-list update-telemetry-list">
                    {updateTelemetry.slice(0, 8).map((entry) => (
                      <article key={entry.id} className="panel-card">
                        <div className="panel-meta">
                          <span>{new Date(entry.at).toLocaleString()}</span>
                          <span>{entry.source}</span>
                          <span>{entry.event}</span>
                        </div>
                        <p>{entry.detail}</p>
                      </article>
                    ))}
                    {updateTelemetry.length === 0 && (
                      <p className="settings-note">No updater events logged yet.</p>
                    )}
                  </div>
                </article>
              </div>
            </section>
          )}
        </div>
      )}

      {showOnboarding && (
        <div className="onboarding-backdrop" data-no-drag>
          <section className="onboarding-modal">
            <span className="settings-kicker">First Launch</span>
            <h2>Welcome to Dictum</h2>
            <p>Set your keybind, transcription defaults, and cloud preference before starting.</p>

            <div className="settings-fields">
              <label className="settings-field">
                <span>Toggle Shortcut</span>
                <input
                  className="settings-input"
                  value={toggleShortcut}
                  onChange={(e) => setToggleShortcut(e.target.value)}
                  placeholder="Ctrl+Shift+Space"
                />
              </label>
              <label className="settings-field">
                <span>Model</span>
                <select className="settings-input" value={modelProfile} onChange={(e) => handleModelProfileChange(e.target.value)}>
                  {MODEL_PROFILE_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </label>
              {recommendedModelMeta && modelRecommendation && (
                <div className="settings-inline-actions">
                  <button className="action-btn" onClick={applyRecommendedModel}>
                    Use Recommended ({recommendedModelMeta.label})
                  </button>
                  <span className="settings-note">{modelRecommendation.reason}</span>
                </div>
              )}
              <label className="settings-field">
                <span>Performance</span>
                <select className="settings-input" value={performanceProfile} onChange={(e) => setPerformanceProfile(e.target.value)}>
                  {PERFORMANCE_PROFILE_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="settings-field">
                <span>Cloud Mode</span>
                <select className="settings-input" value={cloudMode} onChange={(e) => setCloudMode(e.target.value as CloudMode)}>
                  {CLOUD_MODE_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </label>
            </div>

            <label className="settings-field">
              <span>OpenAI API Key (optional)</span>
              <input
                className="settings-input"
                type="password"
                value={openAiApiKeyInput}
                onChange={(e) => setOpenAiApiKeyInput(e.target.value)}
                placeholder={hasOpenAiApiKey ? "Saved locally. Enter new key to replace." : "sk-proj-..."}
                autoComplete="off"
              />
            </label>

            <label className="settings-switch">
              <input type="checkbox" checked={reliabilityMode} onChange={(e) => setReliabilityMode(e.target.checked)} />
              <span>Enable reliability mode (recommended)</span>
            </label>

            <div className="settings-inline-actions">
              <button className="action-btn" onClick={() => void runMicCalibration()} disabled={isCalibrating}>
                {isCalibrating ? "Running diagnostics..." : "Run mic diagnostics"}
              </button>
              <button className="action-btn" onClick={() => void runBenchmarkTune()} disabled={isBenchmarkTuning}>
                {isBenchmarkTuning ? "Benchmarking..." : "Run benchmark tune"}
              </button>
              <button className="action-btn settings-save-btn" onClick={() => void completeOnboarding()}>
                Finish Setup
              </button>
              {onboardingCompleted && (
                <button className="action-btn" onClick={() => setShowOnboarding(false)}>
                  Close
                </button>
              )}
            </div>

            <p className="settings-note">
              {runtimeMsg ?? "You can re-open this onboarding anytime from Settings."}
            </p>
          </section>
        </div>
      )}
    </div>
  );
}
