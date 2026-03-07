"use client";

import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { useActivity } from "@/hooks/useActivity";
import { useAppProfiles } from "@/hooks/useAppProfiles";
import { useAudioDevices } from "@/hooks/useAudioDevices";
import { useCorrectionsManager } from "@/hooks/useCorrectionsManager";
import { useEngine } from "@/hooks/useEngine";
import { useGuidedTune } from "@/hooks/useGuidedTune";
import { usePanelData } from "@/hooks/usePanelData";
import { useUpdateManager } from "@/hooks/useUpdateManager";
import { useTranscript } from "@/hooks/useTranscript";
import { AppUpdatesSection } from "@/components/settings/AppUpdatesSection";
import { LiveCorrectionsSection } from "@/components/settings/LiveCorrectionsSection";
import { OnboardingModal } from "@/components/settings/OnboardingModal";
import { PerAppProfilesSection } from "@/components/settings/PerAppProfilesSection";
import { PrivacySection } from "@/components/settings/PrivacySection";
import type {
  DiagnosticsBundle,
  ModelProfileMetadata,
  ModelProfileRecommendation,
  PrivacySettings,
} from "@shared/ipc_types";
import {
  deleteDictionary,
  deleteHistory,
  deleteSnippet,
  getDiagnosticsBundle,
  exportDiagnosticsBundle,
  getModelProfileCatalog,
  getModelProfileRecommendation,
  getPreferredInputDevice,
  getPrivacySettings,
  getRuntimeSettings,
  runAutoTune,
  setPreferredInputDevice,
  setRuntimeSettings,
  upsertDictionary,
  upsertSnippet,
} from "@/lib/tauri";
import { motion, AnimatePresence } from "framer-motion";
import { smokeBaseline } from "@/data/smokeBaseline";

type Tab = "live" | "history" | "stats" | "dictionary" | "snippets" | "settings";
type CloudMode = "local_only" | "hybrid" | "cloud_preferred";
type DictationMode = "conversation" | "coding" | "command";

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

const formatPct = (value: number | null | undefined): string =>
  value == null ? "n/a" : `${Math.round(value * 100)}%`;

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

const TAB_OPTIONS: Tab[] = ["live", "history", "stats", "dictionary", "snippets", "settings"];

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

const inferCorrectionModeAffinity = (corrected: string, mode: DictationMode): number => {
  const text = corrected.trim();
  if (!text) return 0;
  const hasCodeSymbols = /[_/\\()[\]{}:=;`."'-]/.test(text);
  const hasCamelOrPascal = /[a-z][A-Z]|[A-Z][a-z]+[A-Z]/.test(text);
  const hasDashOrSlash = /[-/]/.test(text);
  const isMostlyLower = text === text.toLowerCase();
  const hasSpaces = /\s/.test(text);

  switch (mode) {
    case "coding":
      return (
        (hasCodeSymbols ? 0.55 : 0) +
        (hasCamelOrPascal ? 0.28 : 0) +
        (!hasSpaces ? 0.12 : 0)
      );
    case "command":
      return (
        (hasDashOrSlash ? 0.45 : 0) +
        (isMostlyLower ? 0.24 : 0) +
        (!hasSpaces ? 0.12 : 0)
      );
    default:
      return (
        (hasSpaces ? 0.2 : 0) +
        (!hasCodeSymbols ? 0.14 : 0) +
        (/^[A-Z]/.test(text) ? 0.08 : 0)
      );
  }
};

const matchesCorrectionContext = (
  rule: {
    modeAffinity?: string | null;
    appProfileAffinity?: string | null;
  },
  activeProfileId: string | null,
  activeMode: DictationMode,
): boolean => {
  if (rule.appProfileAffinity) {
    return rule.appProfileAffinity === activeProfileId;
  }
  if (rule.modeAffinity) {
    return rule.modeAffinity === activeMode;
  }
  return true;
};

const DICTATION_MODE_OPTIONS: Array<{ value: DictationMode; label: string; hint: string }> = [
  {
    value: "conversation",
    label: "Conversation",
    hint: "Sentence-style cleanup for natural prose and messages.",
  },
  {
    value: "coding",
    label: "Coding",
    hint: "Maps common spoken symbol phrases and avoids sentence punctuation.",
  },
  {
    value: "command",
    label: "Command",
    hint: "Lowercases commands, trims punctuation, and favors shell-style text.",
  },
];

const APP_PROFILE_PRESETS: Array<{
  name: string;
  appMatch: string;
  dictationMode: DictationMode;
  phraseBiasTerms: string[];
  postUtteranceRefine: boolean;
}> = [
  {
    name: "Cursor",
    appMatch: "cursor.exe",
    dictationMode: "coding",
    phraseBiasTerms: ["TypeScript", "React", "PostgreSQL"],
    postUtteranceRefine: true,
  },
  {
    name: "VS Code",
    appMatch: "code.exe",
    dictationMode: "coding",
    phraseBiasTerms: ["TypeScript", "JavaScript", "terminal"],
    postUtteranceRefine: true,
  },
  {
    name: "Windows Terminal",
    appMatch: "windowsterminal.exe",
    dictationMode: "command",
    phraseBiasTerms: ["PowerShell", "git", "npm"],
    postUtteranceRefine: false,
  },
  {
    name: "Slack",
    appMatch: "slack.exe",
    dictationMode: "conversation",
    phraseBiasTerms: ["Dictum", "follow-up", "standup"],
    postUtteranceRefine: true,
  },
];

const clamp = (v: number, min: number, max: number): number => Math.min(max, Math.max(min, v));

export default function Home() {
  const { isListening, status, startEngine, stopEngine, error } = useEngine();
  const [activitySensitivity, setActivitySensitivity] = useState(4.2);
  const [activityNoiseGate, setActivityNoiseGate] = useState(0.0015);
  const [activityClipThreshold, setActivityClipThreshold] = useState(0.32);
  const [diagnosticsBundle, setDiagnosticsBundle] = useState<DiagnosticsBundle | null>(null);
  const [diagnosticsLoading, setDiagnosticsLoading] = useState(false);
  const [diagnosticsExporting, setDiagnosticsExporting] = useState(false);
  const [lastDiagnosticsExportPath, setLastDiagnosticsExportPath] = useState<string | null>(null);
  const [readinessChecklistCopied, setReadinessChecklistCopied] = useState<"idle" | "done" | "error">("idle");
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

  const [tab, setTab] = useState<Tab>("live");
  const [copyState, setCopyState] = useState<"idle" | "done" | "error">("idle");
  const [selectedDeviceName, setSelectedDeviceName] = useState<string | null>(null);
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  const [modelProfile, setModelProfile] = useState("distil-large-v3");
  const [performanceProfile, setPerformanceProfile] = useState("whisper_balanced_english");
  const [dictationMode, setDictationMode] = useState<DictationMode>("conversation");
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
  const [activeFixSegmentId, setActiveFixSegmentId] = useState<string | null>(null);
  const [activeFixText, setActiveFixText] = useState("");

  const [historyQuery, setHistoryQuery] = useState("");
  const [modelCatalog, setModelCatalog] = useState<ModelProfileMetadata[]>([]);
  const [modelRecommendation, setModelRecommendation] = useState<ModelProfileRecommendation | null>(null);
  const [dictTerm, setDictTerm] = useState("");
  const [dictAliases, setDictAliases] = useState("");
  const [dictLanguage, setDictLanguage] = useState("");
  const [snippetTrigger, setSnippetTrigger] = useState("");
  const [snippetExpansion, setSnippetExpansion] = useState("");
  const [snippetMode, setSnippetMode] = useState<"slash" | "phrase">("slash");
  const deferredSegments = useDeferredValue(segments);
  const {
    currentAppVersion,
    updateRepoSlug,
    setUpdateRepoSlug,
    updateInfo,
    isCheckingUpdate,
    isInstallingUpdate,
    updateAutoCheckEnabled,
    setUpdateAutoCheckEnabled,
    updateSkipVersion,
    updateRemindUntilMs,
    updateLastCheckedAt,
    updateAutoInstallWhenIdle,
    setUpdateAutoInstallWhenIdle,
    updateTelemetry,
    updateLogCopied,
    handleCheckForUpdates,
    handleInstallUpdate,
    handleRemindLater,
    handleSkipUpdateVersion,
    clearUpdateDeferrals,
    handleExportUpdateTelemetry,
  } = useUpdateManager({
    isListening,
    showOnboarding,
    setRuntimeMsg,
  });
  const {
    appProfiles,
    activeAppContext,
    editingAppProfileId,
    appProfileName,
    setAppProfileName,
    appProfileMatch,
    setAppProfileMatch,
    appProfileMode,
    setAppProfileMode,
    appProfileBiasTerms,
    setAppProfileBiasTerms,
    appProfileRefine,
    setAppProfileRefine,
    appProfilesCopied,
    appProfilesImportText,
    setAppProfilesImportText,
    handleSaveAppProfile,
    handleEditAppProfile,
    resetAppProfileEditor,
    handleApplyAppProfilePreset,
    handleDeleteAppProfile,
    handleCopyAppProfiles,
    handleImportAppProfiles,
  } = useAppProfiles({
    tab,
    setRuntimeMsg,
  });
  const {
    learnedCorrections,
    correctionsCopied,
    correctionsImportText,
    setCorrectionsImportText,
    correctionFilter,
    setCorrectionFilter,
    correctionHeardInput,
    setCorrectionHeardInput,
    correctionFixedInput,
    setCorrectionFixedInput,
    correctionScope,
    setCorrectionScope,
    correctionFilterScope,
    setCorrectionFilterScope,
    correctionSort,
    setCorrectionSort,
    activeCorrectionContext,
    correctionHealthSummary,
    filteredLearnedCorrections,
    editingCorrection,
    applyCorrection,
    handleLearnCorrection,
    handleDeleteCorrection,
    handleStartEditingCorrection,
    handleCancelEditingCorrection,
    handleCopyCorrections,
    handleImportCorrections,
    handlePruneCorrections,
  } = useCorrectionsManager({
    setRuntimeMsg,
    activeAppContext,
    currentDictationMode: dictationMode,
    appProfiles,
  });
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
      deferredSegments
        .filter((seg) => seg.kind === "final" && (seg.confidence ?? 1) < 0.74)
        .slice(-6),
    [deferredSegments],
  );
  const effectiveSuggestionMode = ((activeAppContext?.dictationMode || dictationMode) as DictationMode);
  const correctionSuggestions = useMemo(() => {
    const suggestions: Array<{
      id: string;
      heard: string;
      corrected: string;
      confidence: number;
      score: number;
      scopeLabel: string;
      usageLabel: string;
      lastUsedAt: string | null;
      sourceModeAffinity: string | null;
      sourceAppProfileAffinity: string | null;
    }> = [];
    for (const seg of lowConfidenceFinals) {
      const segConf = seg.confidence ?? 0.6;
      for (const rule of learnedCorrections) {
        const sim = jaccardSimilarity(seg.text, rule.heard);
        const contains = seg.text.toLowerCase().includes(rule.heard.toLowerCase()) ? 1 : 0;
        const modeAffinity = inferCorrectionModeAffinity(rule.corrected, effectiveSuggestionMode);
        const recencyBoost = rule.lastUsedAt
          ? Math.max(0, 0.18 - ((Date.now() - new Date(rule.lastUsedAt).getTime()) / (1000 * 60 * 60 * 24 * 30)) * 0.18)
          : 0;
        const contextBoost =
          (!rule.modeAffinity || rule.modeAffinity === effectiveSuggestionMode ? 0.24 : -0.18) +
          (!rule.appProfileAffinity || rule.appProfileAffinity === activeAppContext?.matchedProfileId ? 0.34 : -0.32);
        const weighted =
          (1 - segConf) *
          (1 + Math.min(rule.hits, 12) / 12) *
          (sim + contains * 0.4 + modeAffinity + contextBoost + recencyBoost);
        if (weighted < 0.18) continue;
        suggestions.push({
          id: `${seg.id}:${rule.heard}:${rule.corrected}`,
          heard: seg.text,
          corrected: rule.corrected,
          confidence: segConf,
          score: weighted,
          scopeLabel: rule.appProfileAffinity
            ? appProfiles.find((profile) => profile.id === rule.appProfileAffinity)?.name || "profile"
            : rule.modeAffinity || "global",
          usageLabel: `hits ${rule.hits}`,
          lastUsedAt: rule.lastUsedAt ?? null,
          sourceModeAffinity: rule.modeAffinity ?? null,
          sourceAppProfileAffinity: rule.appProfileAffinity ?? null,
        });
      }
    }
    suggestions.sort((a, b) => b.score - a.score);
    return suggestions.filter((item, index, arr) => arr.findIndex((other) => other.corrected === item.corrected) === index).slice(0, 5);
  }, [activeAppContext?.matchedProfileId, appProfiles, effectiveSuggestionMode, learnedCorrections, lowConfidenceFinals]);
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
    try {
      const saved = localStorage.getItem("dictum-theme-v1");
      if (saved === "light" || saved === "dark") setTheme(saved);
    } catch {
      // Ignore
    }
  }, []);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    try {
      localStorage.setItem("dictum-theme-v1", theme);
    } catch {
      // Ignore
    }
  }, [theme]);

  useEffect(() => {
    Promise.all([getRuntimeSettings(), getPrivacySettings()])
      .then(([runtime, privacy]) => {
        setModelProfile(runtime.modelProfile || "distil-large-v3");
        setPerformanceProfile(runtime.performanceProfile || "whisper_balanced_english");
        setDictationMode((runtime.dictationMode || "conversation") as DictationMode);
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
    if (copyState === "idle") return;
    const timer = window.setTimeout(() => setCopyState("idle"), 1400);
    return () => window.clearTimeout(timer);
  }, [copyState]);

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

  const refreshDiagnostics = useCallback(async (showErrors = false) => {
    try {
      setDiagnosticsLoading(true);
      const bundle = await getDiagnosticsBundle();
      setDiagnosticsBundle(bundle);
      return bundle;
    } catch (err) {
      if (showErrors) {
        const msg = err instanceof Error ? err.message : String(err);
        setRuntimeMsg(`Failed to load diagnostics: ${msg}`);
      }
      return null;
    } finally {
      setDiagnosticsLoading(false);
    }
  }, []);

  const handleCopyDiagnosticsBundle = useCallback(async () => {
    try {
      const bundle = (await refreshDiagnostics()) ?? (await getDiagnosticsBundle());
      const exportPayload = {
        ...bundle,
        uiDiagnostics: {
          copiedAt: new Date().toISOString(),
          updateTelemetry,
          correctionUiState: {
            scope: correctionScope,
            activeContext: activeCorrectionContext,
            effectiveSuggestionMode,
          },
          updateState: {
            repoSlug: updateRepoSlug.trim() || "sinergaoptima/dictum",
            autoCheckEnabled: updateAutoCheckEnabled,
            autoInstallWhenIdle: updateAutoInstallWhenIdle,
            skipVersion: updateSkipVersion,
            remindUntilMs: updateRemindUntilMs,
            lastCheckedAt: updateLastCheckedAt,
            currentUpdate: updateInfo,
          },
        },
      };
      await navigator.clipboard.writeText(JSON.stringify(exportPayload, null, 2));
      setRuntimeMsg("Copied diagnostics bundle to clipboard.");
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to copy diagnostics bundle: ${msg}`);
    }
  }, [
    updateAutoCheckEnabled,
    updateAutoInstallWhenIdle,
    updateInfo,
    updateLastCheckedAt,
    updateRemindUntilMs,
    updateRepoSlug,
    updateSkipVersion,
    updateTelemetry,
    refreshDiagnostics,
  ]);

  const handleExportDiagnosticsBundle = useCallback(async () => {
    try {
      setDiagnosticsExporting(true);
      const result = await exportDiagnosticsBundle();
      setLastDiagnosticsExportPath(result.path);
      await refreshDiagnostics(false);
      setRuntimeMsg(`Exported diagnostics file to ${result.path}.`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to export diagnostics bundle: ${msg}`);
    } finally {
      setDiagnosticsExporting(false);
    }
  }, [refreshDiagnostics]);

  useEffect(() => {
    if (readinessChecklistCopied === "idle") return;
    const timer = window.setTimeout(() => setReadinessChecklistCopied("idle"), 1600);
    return () => window.clearTimeout(timer);
  }, [readinessChecklistCopied]);

  useEffect(() => {
    if (tab !== "stats") return;
    if (diagnosticsBundle && !diagnosticsLoading) return;
    void refreshDiagnostics(false);
  }, [diagnosticsBundle, diagnosticsLoading, refreshDiagnostics, tab]);

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
      await applyCorrection(
        heard,
        corrected,
        activeCorrectionContext.modeAffinity,
        activeCorrectionContext.appProfileAffinity,
      );
      setCorrectionHeardInput("");
      setCorrectionFixedInput("");
      setActiveFixSegmentId(null);
      setActiveFixText("");
      setRuntimeMsg(`Saved correction: "${heard}" -> "${corrected}".`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to save correction: ${msg}`);
    }
  }, [activeCorrectionContext.appProfileAffinity, activeCorrectionContext.modeAffinity, activeFixText, applyCorrection, setCorrectionFixedInput, setCorrectionHeardInput]);

  const applySuggestionWithScope = useCallback(async (
    heard: string,
    corrected: string,
    scope: "current" | DictationMode | "global" | "profile",
  ) => {
    try {
      let modeAffinity: string | null = null;
      let appProfileAffinity: string | null = null;
      if (scope === "current") {
        modeAffinity = activeCorrectionContext.modeAffinity;
        appProfileAffinity = activeCorrectionContext.appProfileAffinity;
      } else if (scope === "profile") {
        if (!activeAppContext?.matchedProfileId) {
          setRuntimeMsg("No active app profile is matched right now.");
          return;
        }
        modeAffinity = activeAppContext.dictationMode;
        appProfileAffinity = activeAppContext.matchedProfileId;
      } else if (scope === "global") {
        modeAffinity = null;
        appProfileAffinity = null;
      } else {
        modeAffinity = scope;
        appProfileAffinity = null;
      }
      await applyCorrection(
        heard,
        corrected,
        modeAffinity,
        appProfileAffinity,
      );
      setRuntimeMsg(
        `Saved suggestion as ${scope === "current" ? "current context" : scope}: "${heard}" -> "${corrected}".`,
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to apply suggestion: ${msg}`);
    }
  }, [activeAppContext, activeCorrectionContext.appProfileAffinity, activeCorrectionContext.modeAffinity, applyCorrection]);

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
      dictationMode: DictationMode;
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
        dictationMode,
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
          next.dictationMode,
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
        setDictationMode((updated.dictationMode || "conversation") as DictationMode);
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
      dictationMode,
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

  const {
    completeOnboarding,
    runMicCalibration,
    runBenchmarkTune,
    isCalibrating,
    isBenchmarkTuning,
    guidedTuneStepLabel,
    guidedTuneInstruction,
    guidedTuneSentence,
    guidedTuneProgressPct,
    guidedTuneCompletedForDevice,
  } = useGuidedTune({
    selectedDeviceName,
    isListening,
    startEngine,
    stopEngine,
    latestRmsRef,
    applyRuntime,
    setRuntimeMsg,
    setCalibrationMsg,
    setModelProfile,
    setPerformanceProfile,
    setDictationMode,
    setToggleShortcut,
    setOrtEp,
    setOrtIntraThreads,
    setOrtInterThreads,
    setOrtParallel,
    setLanguageHint,
    setPillVisualizerSensitivity,
    setActivitySensitivity,
    setActivityNoiseGate,
    setActivityClipThreshold,
    setInputGainBoost,
    setPostUtteranceRefine,
    setPhraseBiasTerms,
    setCloudMode,
    setCloudOptIn,
    setReliabilityMode,
    setModelRecommendation,
    showOnboarding,
    setShowOnboarding,
  });
  const {
    historyItems,
    stats,
    perfSnapshot,
    dictionary,
    snippets,
    panelLoading,
    refreshHistory,
    refreshStats,
    refreshDictionary,
    refreshSnippets,
  } = usePanelData(tab, historyQuery);

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

  const hasSegments = deferredSegments.length > 0;
  const rmsMeterPercent = clamp(rawRms * 1600, 0, 1) * 100;
  const noiseGatePercent = clamp(activityNoiseGate / 0.03, 0, 1) * 100;
  const clipThresholdPercent = clamp(activityClipThreshold, 0, 1) * 100;
  const fallbackStubRate = perfSnapshot?.diagnostics.finalSegmentsSeen
    ? (perfSnapshot.diagnostics.fallbackStubTyped / perfSnapshot.diagnostics.finalSegmentsSeen) * 100
    : 0;
  const finalizeP95 = perfSnapshot?.finalizeMs.p95Ms ?? 0;
  const inferenceP95 = perfSnapshot?.inferenceMs.p95Ms ?? 0;
  const duplicateFinalSuppressed = perfSnapshot?.diagnostics.duplicateFinalSuppressed ?? 0;
  const partialRescuesUsed = perfSnapshot?.diagnostics.partialRescuesUsed ?? 0;
  const correctionDiagnostics = diagnosticsBundle?.correctionDiagnostics ?? null;
  const settingsHealth = diagnosticsBundle?.settingsHealth ?? null;
  const currentContextRuleCount = correctionHealthSummary.currentContextRules;
  const readinessItems = useMemo(
    () => [
      {
        label: "Guided tune",
        value: guidedTuneCompletedForDevice ? "Ready" : "Needs tune",
        ok: guidedTuneCompletedForDevice,
        detail: guidedTuneCompletedForDevice
          ? "Current mic has completed the guided tune."
          : "Run the 30-second guided voice tune for the current device.",
      },
      {
        label: "Model selection",
        value:
          modelRecommendation && modelRecommendation.recommendedProfile !== modelProfile
            ? "Review recommended"
            : "Aligned",
        ok: !modelRecommendation || modelRecommendation.recommendedProfile === modelProfile,
        detail: modelRecommendation
          ? `Recommended ${modelRecommendation.recommendedProfile}. Current ${modelProfile}.`
          : `Current model ${modelProfile}.`,
      },
      {
        label: "Active profile",
        value: activeAppContext?.matchedProfileName || "No match",
        ok: !!activeAppContext?.matchedProfileName,
        detail: activeAppContext?.foregroundApp
          ? `Foreground ${activeAppContext.foregroundApp}.`
          : "No foreground app detected from the backend.",
      },
      {
        label: "Corrections",
        value: `${learnedCorrections.length} rules`,
        ok:
          learnedCorrections.length > 0 &&
          correctionHealthSummary.orphanedProfileRules === 0,
        detail: `${currentContextRuleCount} rules match the current context. ${correctionHealthSummary.orphanedProfileRules} orphaned, ${correctionHealthSummary.staleRules} stale.`,
      },
      {
        label: "Settings health",
        value: settingsHealth
          ? `Schema v${settingsHealth.currentSchemaVersion}`
          : "Not inspected yet",
        ok: !!settingsHealth,
        detail: settingsHealth
          ? settingsHealth.migrationNotes.length > 0
            ? settingsHealth.migrationNotes.join(" ")
            : `Loaded schema v${settingsHealth.loadedSchemaVersion}. No migration notes were recorded.`
          : "Refresh diagnostics to inspect settings schema and migration notes.",
      },
      {
        label: "Diagnostics export",
        value: lastDiagnosticsExportPath ? "Exported" : "Not exported yet",
        ok: !!lastDiagnosticsExportPath,
        detail: lastDiagnosticsExportPath ?? "Use Export File in Stats to write a local diagnostics bundle.",
      },
      {
        label: "Update path",
        value: updateInfo?.hasUpdate ? `Update ${updateInfo.latestVersion}` : "No pending update",
        ok: true,
        detail: `Repo ${updateRepoSlug.trim() || "sinergaoptima/dictum"}.`,
      },
    ],
    [
      activeAppContext?.foregroundApp,
      activeAppContext?.matchedProfileName,
      correctionHealthSummary.orphanedProfileRules,
      correctionHealthSummary.staleRules,
      currentContextRuleCount,
      guidedTuneCompletedForDevice,
      lastDiagnosticsExportPath,
      learnedCorrections.length,
      modelProfile,
      modelRecommendation,
      settingsHealth,
      updateInfo?.hasUpdate,
      updateInfo?.latestVersion,
      updateRepoSlug,
    ],
  );
  const readinessBlockingItems = useMemo(
    () => readinessItems.filter((item) => !item.ok),
    [readinessItems],
  );
  const readinessChecklistText = useMemo(
    () =>
      [
        "Dictum 0.1.8-dev.5 readiness checklist",
        ...readinessItems.map((item) => `- [${item.ok ? "x" : " "}] ${item.label}: ${item.value} (${item.detail})`),
      ].join("\n"),
    [readinessItems],
  );
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
          {TAB_OPTIONS.map((value) => (
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
        <motion.section
          initial={{ opacity: 0, y: -10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, height: 0 }}
          className="update-banner" data-no-drag
        >
          <div className="update-banner-copy">
            <span className="update-banner-kicker">Update Ready</span>
            <strong>
              Dictum {updateInfo.currentVersion} {"->"} {updateInfo.latestVersion}
            </strong>
            <small>
              {updateInfo.releaseName ?? "New release available"}
              {updatePublishedLabel ? " · " : ""}
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
        </motion.section>
      )}

      {tab === "live" && (
        <>
          <div className="transcript-scroll selectable" ref={feedRef}>
            {hasSegments ? (
              <div className="transcript-feed">
                {deferredSegments.map((seg) => (
                  <div key={seg.id} className={`seg-row has-context-menu ${seg.kind === "partial" ? "is-partial" : "is-final"}`}>
                    <p className={seg.kind === "partial" ? "seg-partial" : "seg-final"}>
                      {seg.text}
                      {seg.kind === "final" && typeof seg.confidence === "number" && (
                        <span className="seg-confidence">
                          {(seg.confidence * 100).toFixed(0)}%
                        </span>
                      )}
                    </p>
                    {seg.kind === "final" && (
                      <div className="context-menu">
                        <button
                          type="button"
                          className="context-action"
                          onClick={() => { navigator.clipboard.writeText(seg.text).catch(console.error); }}
                        >
                          Copy
                        </button>
                        <button
                          type="button"
                          className="context-action"
                          onClick={() => startInlineFix(seg.id, seg.text)}
                        >
                          Fix
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
                    <span className="suggestion-label">
                      Likely corrections · {effectiveSuggestionMode}
                    </span>
                    {correctionSuggestions.map((s) => (
                      <div
                        key={s.id}
                        className="suggestion-chip"
                      >
                        <button
                          type="button"
                          className="suggestion-chip-main"
                          onClick={() => void applySuggestionWithScope(s.heard, s.corrected, "current")}
                          title={`${s.scopeLabel} · ${s.usageLabel} · confidence ${(s.confidence * 100).toFixed(0)}%${s.lastUsedAt ? ` · used ${new Date(s.lastUsedAt).toLocaleString()}` : ""}`}
                        >
                          <span>{s.corrected}</span>
                          <small>{s.scopeLabel} · {s.usageLabel}</small>
                        </button>
                        <div className="suggestion-actions">
                          <button type="button" className="suggestion-scope-btn" onClick={() => void applySuggestionWithScope(s.heard, s.corrected, "global")}>
                            Global
                          </button>
                          <button type="button" className="suggestion-scope-btn" onClick={() => void applySuggestionWithScope(s.heard, s.corrected, effectiveSuggestionMode)}>
                            {effectiveSuggestionMode}
                          </button>
                          <button
                            type="button"
                            className="suggestion-scope-btn"
                            onClick={() => void applySuggestionWithScope(s.heard, s.corrected, "profile")}
                            disabled={!activeAppContext?.matchedProfileId}
                            title={activeAppContext?.matchedProfileName ? `Save only for ${activeAppContext.matchedProfileName}` : "No active app profile"}
                          >
                            {activeAppContext?.matchedProfileName ? "Profile" : "No Profile"}
                          </button>
                        </div>
                      </div>
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
                {Array.from({ length: 9 }).map((_, i) => {
                  const distance = Math.abs(i - 4);
                  const falloff = 1 - (distance * 0.22);
                  const baseHeight = 4 + (distance * 1.5);
                  const activeHeight = baseHeight + (level * 22 * falloff);
                  return (
                    <motion.span
                      key={i}
                      className="level-bar"
                      animate={{
                        height: activeHeight,
                        opacity: 0.35 + (level * 0.65 * falloff),
                        backgroundColor: level > 0.05 ? "rgb(var(--accent))" : "rgba(var(--border), 0.9)"
                      }}
                      transition={{ type: "spring", bounce: 0.25, duration: 0.35 }}
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

      <AnimatePresence mode="wait">
        {tab !== "live" && (
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 10 }}
            transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
            className="panel-scroll selectable over-panel"
            data-no-drag
          >


            <div className="panel-content-glass">
          {tab === "history" && (
            <section className="panel">
              <div className="panel-toolbar">
                <input
                  className="runtime-select panel-input"
                  placeholder="Search dictation history..."
                  value={historyQuery}
                  onChange={(e) => setHistoryQuery(e.target.value)}
                />
                <button
                  className="action-btn"
                  onClick={() => void refreshHistory(true, historyQuery.trim())}
                  disabled={panelLoading.history}
                >
                  {panelLoading.history ? "Refreshing..." : "Refresh"}
                </button>
                <button
                  className="action-btn"
                  onClick={async () => {
                    await deleteHistory(null, retentionDays);
                    await refreshHistory(true, historyQuery.trim());
                  }}
                  disabled={panelLoading.history}
                >
                  Prune
                </button>
              </div>
              <div className="panel-list">
                {panelLoading.history && historyItems.length === 0 && (
                  <div className="empty-slate">
                    <span className="empty-icon" aria-hidden="true">◌</span>
                    <h3>Loading history</h3>
                    <p>Pulling your recent dictation sessions.</p>
                  </div>
                )}
                {historyItems.map((item, i) => (
                  <motion.article
                    key={item.id}
                    className="panel-card history-card has-context-menu"
                    initial={{ opacity: 0, scale: 0.98, y: 10 }}
                    animate={{ opacity: 1, scale: 1, y: 0 }}
                    transition={{ duration: 0.3, delay: i * 0.03, ease: [0.16, 1, 0.3, 1] }}
                  >
                    <div className="panel-meta">
                      <span>{new Date(item.createdAt).toLocaleString()}</span>
                      <span>{item.source}</span>
                      <span>{item.wordCount} words</span>
                      {item.latencyMs > 0 && <span>{item.latencyMs} ms</span>}
                    </div>
                    <p>{item.text}</p>
                    <div className="context-menu">
                      <button className="context-action" onClick={(e) => { e.stopPropagation(); navigator.clipboard.writeText(item.text).catch(console.error); }}>Copy</button>
                      <button className="context-action danger" onClick={async (e) => { e.stopPropagation(); await deleteHistory([item.id], null); await refreshHistory(true, historyQuery.trim()); }}>Delete</button>
                    </div>
                  </motion.article>
                ))}
                {!panelLoading.history && historyItems.length === 0 && (
                  <div className="empty-slate">
                    <span className="empty-icon" aria-hidden="true">◷</span>
                    <h3>{historyQuery.trim() ? "No matches" : "No history yet"}</h3>
                    <p>
                      {historyQuery.trim()
                        ? "Try a broader search term or clear the filter."
                        : "Your dictated sessions will be safely archived here for review."}
                    </p>
                  </div>
                )}
              </div>
            </section>
          )}

          {tab === "stats" && (
            <section className="panel">
              <div className="panel-toolbar">
                <button
                  className="action-btn"
                  onClick={() => void (async () => {
                    await Promise.all([refreshStats(true), refreshDiagnostics(true)]);
                  })()}
                  disabled={panelLoading.stats || diagnosticsLoading}
                >
                  {panelLoading.stats || diagnosticsLoading ? "Refreshing..." : "Refresh"}
                </button>
                <button className="action-btn" onClick={() => void handleCopyDiagnosticsBundle()}>
                  Copy Diagnostics
                </button>
                <button
                  className="action-btn"
                  onClick={() => void handleExportDiagnosticsBundle()}
                  disabled={diagnosticsExporting}
                >
                  {diagnosticsExporting ? "Exporting..." : "Export File"}
                </button>
              </div>
              {panelLoading.stats && !stats ? (
                <div className="empty-slate">
                  <span className="empty-icon" aria-hidden="true">◌</span>
                  <h3>Loading stats</h3>
                  <p>Gathering recent usage and performance telemetry.</p>
                </div>
              ) : stats ? (
                <>
                  <motion.div
                    className="panel-grid"
                    initial="hidden"
                    animate="visible"
                    variants={{
                      hidden: { opacity: 0 },
                      visible: { opacity: 1, transition: { staggerChildren: 0.05 } }
                    }}
                  >
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{stats.totalUtterances}</b><span>Utterances</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{stats.totalWords}</b><span>Words</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{Math.round(stats.avgLatencyMs)} ms</b><span>Avg Latency</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{`${Math.round(fallbackStubRate)}%`}</b><span>Fallback Stub Rate</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{perfSnapshot?.diagnostics.shortcutToggleDropped ?? 0}</b><span>Shortcut Drops</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{Math.round(finalizeP95)} ms</b><span>Finalize p95</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{Math.round(inferenceP95)} ms</b><span>Inference p95</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{duplicateFinalSuppressed}</b><span>Duplicate Finals Blocked</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{partialRescuesUsed}</b><span>Partial Rescues</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{correctionDiagnostics?.totalRules ?? learnedCorrections.length}</b><span>Correction Rules</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{currentContextRuleCount}</b><span>Current Context Rules</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{correctionDiagnostics?.unusedRules ?? 0}</b><span>Unused Rules</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{correctionDiagnostics?.orphanedProfileRules ?? correctionHealthSummary.orphanedProfileRules}</b><span>Orphaned Profile Rules</span>
                    </motion.div>
                    <motion.div className="stat-card" variants={{ hidden: { opacity: 0, scale: 0.9, y: 10 }, visible: { opacity: 1, scale: 1, y: 0 } }}>
                      <b>{correctionDiagnostics?.staleRules ?? correctionHealthSummary.staleRules}</b><span>Stale Rules</span>
                    </motion.div>
                  </motion.div>
                  {correctionDiagnostics && (
                  <div className="settings-stack">
                    <article className="settings-card">
                      <div className="settings-card-header">
                        <h3>Correction Diagnostics</h3>
                        <p>Rule coverage for the current context and the correction rules Dictum is leaning on most.</p>
                      </div>
                      <div className="panel-grid">
                        <div className="stat-card">
                          <b>{correctionDiagnostics.globalRules}</b><span>Global Rules</span>
                        </div>
                        <div className="stat-card">
                          <b>{correctionDiagnostics.modeScopedRules}</b><span>Mode Rules</span>
                        </div>
                        <div className="stat-card">
                          <b>{correctionDiagnostics.profileScopedRules}</b><span>Profile Rules</span>
                        </div>
                        <div className="stat-card">
                          <b>{correctionDiagnostics.orphanedProfileRules}</b><span>Orphaned Rules</span>
                        </div>
                        <div className="stat-card">
                          <b>{correctionDiagnostics.staleRules}</b><span>Stale Rules</span>
                        </div>
                        <div className="stat-card">
                          <b>{diagnosticsBundle?.activeAppContext.foregroundApp || "none"}</b><span>Foreground App</span>
                        </div>
                        <div className="stat-card">
                          <b>{diagnosticsBundle?.activeAppContext.matchedProfileName || "none"}</b><span>Matched Profile</span>
                        </div>
                        <div className="stat-card">
                          <b>{diagnosticsBundle?.activeAppContext.dictationMode || effectiveSuggestionMode}</b><span>Effective Mode</span>
                        </div>
                        <div className="stat-card">
                          <b>{diagnosticsBundle?.activeAppContext.phraseBiasTermCount ?? 0}</b><span>Bias Terms</span>
                        </div>
                      </div>
                      <div className="settings-split-grid">
                        <div className="panel-list">
                          <p className="settings-note">Most used rules</p>
                          {correctionDiagnostics.topRules.slice(0, 5).map((rule) => (
                            <article
                              key={`top:${rule.heard}:${rule.corrected}:${rule.modeAffinity || "any"}:${rule.appProfileAffinity || "all"}`}
                              className={`panel-card ${matchesCorrectionContext(rule, activeAppContext?.matchedProfileId ?? null, effectiveSuggestionMode) ? "is-active" : ""}`}
                            >
                              <div className="panel-meta">
                                <span>{rule.heard}</span>
                                <span>→</span>
                                <span>{rule.corrected}</span>
                                <span>hits {rule.hits}</span>
                              </div>
                              <p>
                                {rule.appProfileName
                                  ? `Profile ${rule.appProfileName}`
                                  : rule.modeAffinity
                                    ? `Mode ${rule.modeAffinity}`
                                    : "Global"}
                                {rule.lastUsedAt ? ` · used ${new Date(rule.lastUsedAt).toLocaleString()}` : ""}
                              </p>
                            </article>
                          ))}
                        </div>
                        <div className="panel-list">
                          <p className="settings-note">Recently active rules</p>
                          {correctionDiagnostics.recentRules.slice(0, 5).map((rule) => (
                            <article
                              key={`recent:${rule.heard}:${rule.corrected}:${rule.modeAffinity || "any"}:${rule.appProfileAffinity || "all"}`}
                              className={`panel-card ${matchesCorrectionContext(rule, activeAppContext?.matchedProfileId ?? null, effectiveSuggestionMode) ? "is-active" : ""}`}
                            >
                              <div className="panel-meta">
                                <span>{rule.heard}</span>
                                <span>→</span>
                                <span>{rule.corrected}</span>
                                <span>hits {rule.hits}</span>
                              </div>
                              <p>
                                {rule.appProfileName
                                  ? `Profile ${rule.appProfileName}`
                                  : rule.modeAffinity
                                    ? `Mode ${rule.modeAffinity}`
                                    : "Global"}
                                {rule.lastUsedAt ? ` · used ${new Date(rule.lastUsedAt).toLocaleString()}` : ""}
                              </p>
                            </article>
                          ))}
                          {correctionDiagnostics.recentRules.length === 0 && (
                            <p className="settings-note">No corrections have been used in live dictation yet.</p>
                          )}
                        </div>
                      </div>
                    </article>
                  </div>
                  )}
                  {settingsHealth && (
                  <div className="settings-stack">
                    <article className="settings-card">
                      <div className="settings-card-header">
                        <h3>Settings Health</h3>
                        <p>Schema and migration visibility for the local settings file used by this install.</p>
                      </div>
                      <div className="panel-grid">
                        <div className="stat-card">
                          <b>v{settingsHealth.loadedSchemaVersion}</b><span>Loaded Schema</span>
                        </div>
                        <div className="stat-card">
                          <b>v{settingsHealth.currentSchemaVersion}</b><span>Current Schema</span>
                        </div>
                        <div className="stat-card">
                          <b>{settingsHealth.migrationApplied ? "yes" : "no"}</b><span>Migration Applied</span>
                        </div>
                        <div className="stat-card">
                          <b>{settingsHealth.migrationNotes.length}</b><span>Migration Notes</span>
                        </div>
                      </div>
                      <div className="panel-list">
                        {settingsHealth.migrationNotes.length > 0 ? (
                          settingsHealth.migrationNotes.map((note) => (
                            <article key={note} className="panel-card">
                              <p>{note}</p>
                            </article>
                          ))
                        ) : (
                          <p className="settings-note">No settings migration notes were recorded for this load.</p>
                        )}
                      </div>
                    </article>
                  </div>
                  )}
                  <div className="settings-stack">
                    <article className="settings-card">
                      <div className="settings-card-header">
                        <h3>Release Readiness</h3>
                        <p>Quick stabilization readout for the current dev branch before another cut or installer build.</p>
                      </div>
                      <div className="settings-inline-actions">
                        <button
                          className="action-btn"
                          onClick={() => void (async () => {
                            try {
                              await navigator.clipboard.writeText(readinessChecklistText);
                              setReadinessChecklistCopied("done");
                              setRuntimeMsg("Copied readiness checklist to clipboard.");
                            } catch (err) {
                              const msg = err instanceof Error ? err.message : String(err);
                              setReadinessChecklistCopied("error");
                              setRuntimeMsg(`Failed to copy readiness checklist: ${msg}`);
                            }
                          })()}
                          type="button"
                        >
                          {readinessChecklistCopied === "done"
                            ? "Checklist Copied"
                            : readinessChecklistCopied === "error"
                              ? "Copy Failed"
                              : "Copy Checklist"}
                        </button>
                        <span className="settings-note">
                          {readinessBlockingItems.length === 0
                            ? "No blocking readiness items are currently flagged."
                            : `${readinessBlockingItems.length} blocking item${readinessBlockingItems.length === 1 ? "" : "s"} still need attention.`}
                        </span>
                      </div>
                      <div className="settings-split-grid">
                        {readinessItems.map((item) => (
                          <article
                            key={item.label}
                            className={`panel-card ${item.ok ? "is-active" : "is-editing"}`}
                          >
                            <div className="panel-meta">
                              <span>{item.label}</span>
                              <span>{item.value}</span>
                            </div>
                            <p>{item.detail}</p>
                          </article>
                        ))}
                      </div>
                    </article>
                    <article className="settings-card">
                      <div className="settings-card-header">
                        <h3>Smoke Benchmark Baseline</h3>
                        <p>
                          Repo baseline from the committed smoke fixture pack. Use it as a stable reference when tuning `0.1.8-dev.5`.
                        </p>
                      </div>
                      <div className="panel-grid">
                        <div className="stat-card">
                          <b>{smokeBaseline.totalFiles}</b><span>Fixture Files</span>
                        </div>
                        <div className="stat-card">
                          <b>{Math.round(smokeBaseline.p95LatencyMs)} ms</b><span>Repo p95 Latency</span>
                        </div>
                        <div className="stat-card">
                          <b>{formatPct(smokeBaseline.missRate)}</b><span>Repo Miss Rate</span>
                        </div>
                        <div className="stat-card">
                          <b>{formatPct(smokeBaseline.avgSimilarityToExpected)}</b><span>Expected Similarity</span>
                        </div>
                      </div>
                      <div className="panel-list">
                        {smokeBaseline.categories.map((category) => (
                          <article key={category.category} className="panel-card">
                            <div className="panel-meta">
                              <span>{category.category}</span>
                              <span>{category.runs} run{category.runs === 1 ? "" : "s"}</span>
                              <span>p95 {Math.round(category.p95LatencyMs)} ms</span>
                              <span>miss {formatPct(category.missRate)}</span>
                            </div>
                            <p>
                              confidence {formatPct(category.avgConfidence)} · similarity {formatPct(category.avgSimilarityToExpected)} · placeholder {formatPct(category.placeholderRate)}
                            </p>
                          </article>
                        ))}
                      </div>
                    </article>
                  </div>
                </>
              ) : (
                <div className="empty-slate">
                  <span className="empty-icon" aria-hidden="true">⚗</span>
                  <h3>No stats yet</h3>
                  <p>Begin dictating to see your system performance charts here.</p>
                </div>
              )}
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
                    await refreshDictionary(true);
                  }}
                  disabled={panelLoading.dictionary}
                >
                  {panelLoading.dictionary ? "Saving..." : "Add"}
                </button>
              </div>
              <div className="panel-list">
                {panelLoading.dictionary && dictionary.length === 0 && (
                  <div className="empty-slate">
                    <span className="empty-icon" aria-hidden="true">◌</span>
                    <h3>Loading dictionary</h3>
                    <p>Fetching your custom vocabulary.</p>
                  </div>
                )}
                {dictionary.map((entry, i) => (
                  <motion.article
                    key={entry.id}
                    className="panel-card dict-card has-context-menu"
                    initial={{ opacity: 0, scale: 0.98, y: 10 }}
                    animate={{ opacity: 1, scale: 1, y: 0 }}
                    transition={{ duration: 0.3, delay: i * 0.03, ease: [0.16, 1, 0.3, 1] }}
                  >
                    <div className="panel-meta">
                      <span>{entry.term}</span>
                      <span>{entry.language ?? "any"}</span>
                    </div>
                    <p>{entry.aliases.join(", ") || "No aliases"}</p>
                    <div className="context-menu">
                      <button className="context-action danger" onClick={async (e) => { e.stopPropagation(); await deleteDictionary(entry.id); await refreshDictionary(true); }}>Delete</button>
                    </div>
                  </motion.article>
                ))}
                {!panelLoading.dictionary && dictionary.length === 0 && (
                  <div className="empty-slate">
                    <span className="empty-icon" aria-hidden="true">¶</span>
                    <h3>No dictionary entries</h3>
                    <p>Add custom vocabulary to help Dictum recognize specialized terms.</p>
                  </div>
                )}
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
                    await refreshSnippets(true);
                  }}
                  disabled={panelLoading.snippets}
                >
                  {panelLoading.snippets ? "Saving..." : "Add"}
                </button>
              </div>
              <div className="panel-list">
                {panelLoading.snippets && snippets.length === 0 && (
                  <div className="empty-slate">
                    <span className="empty-icon" aria-hidden="true">◌</span>
                    <h3>Loading snippets</h3>
                    <p>Fetching your saved phrase expansions.</p>
                  </div>
                )}
                {snippets.map((entry, i) => (
                  <motion.article
                    key={entry.id}
                    className="panel-card snippet-card has-context-menu"
                    initial={{ opacity: 0, scale: 0.98, y: 10 }}
                    animate={{ opacity: 1, scale: 1, y: 0 }}
                    transition={{ duration: 0.3, delay: i * 0.03, ease: [0.16, 1, 0.3, 1] }}
                  >
                    <div className="panel-meta">
                      <span>{entry.trigger}</span>
                      <span>{entry.mode}</span>
                    </div>
                    <p>{entry.expansion}</p>
                    <div className="context-menu">
                      <button className="context-action danger" onClick={async (e) => { e.stopPropagation(); await deleteSnippet(entry.id); await refreshSnippets(true); }}>Delete</button>
                    </div>
                  </motion.article>
                ))}
                {!panelLoading.snippets && snippets.length === 0 && (
                  <div className="empty-slate">
                    <span className="empty-icon" aria-hidden="true">⚡</span>
                    <h3>No snippets</h3>
                    <p>Create text expansions that automatically trigger when you dictate.</p>
                  </div>
                )}
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
                    {isBenchmarkTuning ? "Benchmarking..." : "Advanced Benchmark Tune"}
                  </button>
                  <button className="action-btn" onClick={() => void runMicCalibration()} disabled={isCalibrating}>
                    {isCalibrating ? "Guided Tune Running..." : "Guided Voice Tune"}
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
                    <h3>Appearance</h3>
                    <p>Switch between dark and light themes.</p>
                  </div>
                  <div className="settings-chip-row">
                    <button
                      className={`settings-chip-btn${theme === "dark" ? " settings-save-btn" : ""}`}
                      onClick={() => setTheme("dark")}
                    >
                      Dark
                    </button>
                    <button
                      className={`settings-chip-btn${theme === "light" ? " settings-save-btn" : ""}`}
                      onClick={() => setTheme("light")}
                    >
                      Light
                    </button>
                  </div>
                  <p className="settings-note">
                    {theme === "dark" ? "Purple accent with dark palette." : "Teal accent with light palette."}
                  </p>
                </article>

                <article className="settings-card settings-card-accent">
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
                      <span>Dictation Mode</span>
                      <select
                        className="settings-input"
                        value={dictationMode}
                        onChange={(e) => setDictationMode(e.target.value as DictationMode)}
                      >
                        {DICTATION_MODE_OPTIONS.map((opt) => (
                          <option key={opt.value} value={opt.value}>
                            {opt.label}
                          </option>
                        ))}
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

                <article className="settings-card settings-card-accent">
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
                  <p className="settings-note">
                    {DICTATION_MODE_OPTIONS.find((opt) => opt.value === dictationMode)?.hint}
                  </p>
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

                <PerAppProfilesSection
                  profiles={appProfiles}
                  activeAppContext={activeAppContext}
                  globalDictationMode={dictationMode}
                  globalPhraseBiasCount={phraseBiasTerms.split(/\r?\n|,/).filter(Boolean).length}
                  profileName={appProfileName}
                  profileMatch={appProfileMatch}
                  profileMode={appProfileMode}
                  profileBiasTerms={appProfileBiasTerms}
                  profileRefine={appProfileRefine}
                  editingProfileId={editingAppProfileId}
                  importText={appProfilesImportText}
                  copiedState={appProfilesCopied}
                  modeOptions={DICTATION_MODE_OPTIONS}
                  presets={APP_PROFILE_PRESETS}
                  onNameChange={setAppProfileName}
                  onMatchChange={setAppProfileMatch}
                  onModeChange={setAppProfileMode}
                  onBiasTermsChange={setAppProfileBiasTerms}
                  onRefineChange={setAppProfileRefine}
                  onImportTextChange={setAppProfilesImportText}
                  onSave={handleSaveAppProfile}
                  onCopy={handleCopyAppProfiles}
                  onImport={handleImportAppProfiles}
                  onEdit={handleEditAppProfile}
                  onDelete={handleDeleteAppProfile}
                  onCancelEdit={resetAppProfileEditor}
                  onApplyPreset={handleApplyAppProfilePreset}
                />

                <LiveCorrectionsSection
                  correctionHeardInput={correctionHeardInput}
                  correctionFixedInput={correctionFixedInput}
                  correctionsCopied={correctionsCopied}
                  correctionFilter={correctionFilter}
                  correctionsImportText={correctionsImportText}
                  learnedCorrections={learnedCorrections}
                  filteredLearnedCorrections={filteredLearnedCorrections}
                  activeAppContext={activeAppContext}
                  appProfiles={appProfiles}
                  currentDictationMode={dictationMode}
                  correctionScope={correctionScope}
                  correctionFilterScope={correctionFilterScope}
                  correctionSort={correctionSort}
                  correctionHealthSummary={correctionHealthSummary}
                  editingCorrection={editingCorrection}
                  onCorrectionHeardInputChange={setCorrectionHeardInput}
                  onCorrectionFixedInputChange={setCorrectionFixedInput}
                  onCorrectionFilterChange={setCorrectionFilter}
                  onCorrectionsImportTextChange={setCorrectionsImportText}
                  onCorrectionScopeChange={setCorrectionScope}
                  onCorrectionFilterScopeChange={setCorrectionFilterScope}
                  onCorrectionSortChange={setCorrectionSort}
                  onLearnCorrection={handleLearnCorrection}
                  onCopyCorrections={handleCopyCorrections}
                  onImportCorrections={handleImportCorrections}
                  onPruneCorrections={handlePruneCorrections}
                  onDeleteCorrection={handleDeleteCorrection}
                  onStartEditingCorrection={handleStartEditingCorrection}
                  onCancelEditingCorrection={handleCancelEditingCorrection}
                />

                <PrivacySection
                  openAiApiKeyInput={openAiApiKeyInput}
                  hasOpenAiApiKey={hasOpenAiApiKey}
                  cloudMode={cloudMode}
                  cloudModeOptions={CLOUD_MODE_OPTIONS}
                  historyEnabled={historyEnabled}
                  guidedTuneCompletedForDevice={guidedTuneCompletedForDevice}
                  guidedTuneProgressPct={guidedTuneProgressPct}
                  guidedTuneStepLabel={guidedTuneStepLabel ?? "Ready to calibrate"}
                  guidedTuneInstruction={guidedTuneInstruction ?? "Press Guided Voice Tune to start the scripted calibration."}
                  guidedTuneSentence={guidedTuneSentence}
                  runtimeMsg={runtimeMsg}
                  calibrationMsg={calibrationMsg}
                  onOpenAiApiKeyInputChange={setOpenAiApiKeyInput}
                  onCloudModeChange={setCloudMode}
                  onHistoryEnabledChange={setHistoryEnabled}
                  onClearSavedKey={() => applyRuntime({ openAiApiKey: "" })}
                />

                <AppUpdatesSection
                  updateAutoCheckEnabled={updateAutoCheckEnabled}
                  updateAutoInstallWhenIdle={updateAutoInstallWhenIdle}
                  updateRepoSlug={updateRepoSlug}
                  isCheckingUpdate={isCheckingUpdate}
                  isInstallingUpdate={isInstallingUpdate}
                  updateInfo={updateInfo}
                  updateLogCopied={updateLogCopied}
                  updateLastCheckedAt={updateLastCheckedAt}
                  updateSkipVersion={updateSkipVersion}
                  updateRemindUntilMs={updateRemindUntilMs}
                  currentAppVersion={currentAppVersion}
                  updateTelemetry={updateTelemetry}
                  onUpdateAutoCheckEnabledChange={setUpdateAutoCheckEnabled}
                  onUpdateAutoInstallWhenIdleChange={setUpdateAutoInstallWhenIdle}
                  onUpdateRepoSlugChange={setUpdateRepoSlug}
                  onCheckForUpdates={() => handleCheckForUpdates({ silent: false, ignoreDeferrals: true })}
                  onInstallUpdate={() => handleInstallUpdate({ source: "manual", autoExit: false })}
                  onRemindLater={handleRemindLater}
                  onSkipThisVersion={handleSkipUpdateVersion}
                  onClearDeferrals={clearUpdateDeferrals}
                  onExportUpdateTelemetry={handleExportUpdateTelemetry}
                />
              </div>
            </section>
          )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <OnboardingModal
        visible={showOnboarding}
        toggleShortcut={toggleShortcut}
        modelProfile={modelProfile}
        performanceProfile={performanceProfile}
        dictationMode={dictationMode}
        cloudMode={cloudMode}
        openAiApiKeyInput={openAiApiKeyInput}
        hasOpenAiApiKey={hasOpenAiApiKey}
        reliabilityMode={reliabilityMode}
        onboardingCompleted={onboardingCompleted}
        guidedTuneCompletedForDevice={guidedTuneCompletedForDevice}
        guidedTuneProgressPct={guidedTuneProgressPct}
        guidedTuneStepLabel={guidedTuneStepLabel ?? "About 30 seconds"}
        guidedTuneInstruction={
          guidedTuneInstruction ??
          "This is required for first-run setup so whisper and normal speech both behave correctly."
        }
        guidedTuneSentence={guidedTuneSentence}
        runtimeMsg={runtimeMsg}
        calibrationMsg={calibrationMsg}
        isCalibrating={isCalibrating}
        isBenchmarkTuning={isBenchmarkTuning}
        modelProfileOptions={MODEL_PROFILE_OPTIONS}
        performanceProfileOptions={PERFORMANCE_PROFILE_OPTIONS}
        dictationModeOptions={DICTATION_MODE_OPTIONS}
        cloudModeOptions={CLOUD_MODE_OPTIONS}
        recommendedModelLabel={recommendedModelMeta?.label ?? null}
        modelRecommendationReason={modelRecommendation?.reason ?? null}
        onToggleShortcutChange={setToggleShortcut}
        onModelProfileChange={handleModelProfileChange}
        onApplyRecommendedModel={applyRecommendedModel}
        onPerformanceProfileChange={setPerformanceProfile}
        onDictationModeChange={setDictationMode}
        onCloudModeChange={setCloudMode}
        onOpenAiApiKeyInputChange={setOpenAiApiKeyInput}
        onReliabilityModeChange={setReliabilityMode}
        onRunMicCalibration={runMicCalibration}
        onRunBenchmarkTune={runBenchmarkTune}
        onCompleteOnboarding={completeOnboarding}
        onClose={() => setShowOnboarding(false)}
      />
    </div>
  );
}
