"use client";

import { useCallback, useEffect, useState } from "react";
import type { PerfSnapshot, RuntimeSettings } from "@shared/ipc_types";
import { getPerfSnapshot, getModelProfileRecommendation, runBenchmarkAutoTune } from "@/lib/tauri";

type CloudMode = "local_only" | "hybrid" | "cloud_preferred";
type DictationMode = "conversation" | "coding" | "command";

type MicCalibration = {
  pillVisualizerSensitivity: number;
  activitySensitivity: number;
  activityNoiseGate: number;
  activityClipThreshold: number;
  inputGainBoost: number;
  guidedTuneVersion?: number;
  calibratedAt?: string;
};

type GuidedTuneStage = {
  key: string;
  mode: "ambient" | "normal" | "whisper";
  label: string;
  instruction: string;
  sentence?: string;
  prepMs?: number;
  captureMs: number;
};

type ApplyRuntimeFn = (overrides?: Partial<{
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
}>) => Promise<boolean>;

type UseGuidedTuneOptions = {
  selectedDeviceName: string | null;
  isListening: boolean;
  startEngine: (deviceName: string | null) => Promise<void>;
  stopEngine: () => Promise<void>;
  latestRmsRef: React.MutableRefObject<number>;
  applyRuntime: ApplyRuntimeFn;
  setRuntimeMsg: (msg: string | null) => void;
  setCalibrationMsg: (msg: string | null) => void;
  setModelProfile: (value: string) => void;
  setPerformanceProfile: (value: string) => void;
  setDictationMode: (value: DictationMode) => void;
  setToggleShortcut: (value: string) => void;
  setOrtEp: (value: string) => void;
  setOrtIntraThreads: (value: number) => void;
  setOrtInterThreads: (value: number) => void;
  setOrtParallel: (value: boolean) => void;
  setLanguageHint: (value: string) => void;
  setPillVisualizerSensitivity: (value: number) => void;
  setActivitySensitivity: (value: number) => void;
  setActivityNoiseGate: (value: number) => void;
  setActivityClipThreshold: (value: number) => void;
  setInputGainBoost: (value: number) => void;
  setPostUtteranceRefine: (value: boolean) => void;
  setPhraseBiasTerms: (value: string) => void;
  setCloudMode: (value: CloudMode) => void;
  setCloudOptIn: (value: boolean) => void;
  setReliabilityMode: (value: boolean) => void;
  setModelRecommendation: (value: any) => void;
  showOnboarding: boolean;
  setShowOnboarding: (value: boolean) => void;
};

const MIC_CALIBRATION_STORAGE_KEY = "dictum-mic-calibration-v1";
const GUIDED_TUNE_VERSION = 2;
const GUIDED_TUNE_TOTAL_MS = 30_000;
const GUIDED_TUNE_STAGES: GuidedTuneStage[] = [
  {
    key: "ambient",
    mode: "ambient",
    label: "Room Tone",
    instruction: "Stay quiet for a few seconds so Dictum can hear the room.",
    captureMs: 4_000,
  },
  {
    key: "normal_one",
    mode: "normal",
    label: "Normal Voice 1",
    instruction: "Read this at your normal speaking volume.",
    sentence: "Dictum should catch my normal voice in a steady speaking rhythm.",
    prepMs: 1_000,
    captureMs: 5_000,
  },
  {
    key: "whisper_one",
    mode: "whisper",
    label: "Whisper Voice 1",
    instruction: "Now whisper this sentence softly but clearly.",
    sentence: "Even when I whisper, I want the transcript to stay reliable and clear.",
    prepMs: 1_000,
    captureMs: 5_000,
  },
  {
    key: "normal_two",
    mode: "normal",
    label: "Normal Voice 2",
    instruction: "One more normal-volume sentence.",
    sentence: "This second sample helps tune both stability and speed for everyday dictation.",
    prepMs: 1_000,
    captureMs: 5_000,
  },
  {
    key: "whisper_two",
    mode: "whisper",
    label: "Whisper Voice 2",
    instruction: "Finish with one more whisper sample.",
    sentence: "Quiet speech should still trigger dictation without forcing me to speak loudly.",
    prepMs: 1_000,
    captureMs: 5_000,
  },
];

const clamp = (v: number, min: number, max: number): number => Math.min(max, Math.max(min, v));
const percentile = (samples: number[], p: number): number => {
  if (samples.length === 0) return 0;
  const sorted = [...samples].sort((a, b) => a - b);
  const idx = Math.floor((sorted.length - 1) * clamp(p, 0, 1));
  return sorted[idx] ?? 0;
};

const loadCalibrationProfiles = (): Record<string, MicCalibration> => {
  try {
    const raw = localStorage.getItem(MIC_CALIBRATION_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, MicCalibration>;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
};

const saveCalibrationProfile = (deviceName: string, profile: MicCalibration): void => {
  try {
    const all = loadCalibrationProfiles();
    all[deviceName] = profile;
    localStorage.setItem(MIC_CALIBRATION_STORAGE_KEY, JSON.stringify(all));
  } catch {}
};

const applyRuntimeSettings = (
  runtime: RuntimeSettings,
  setters: Pick<UseGuidedTuneOptions,
    | "setModelProfile"
    | "setPerformanceProfile"
    | "setDictationMode"
    | "setToggleShortcut"
    | "setOrtEp"
    | "setOrtIntraThreads"
    | "setOrtInterThreads"
    | "setOrtParallel"
    | "setLanguageHint"
    | "setPillVisualizerSensitivity"
    | "setActivitySensitivity"
    | "setActivityNoiseGate"
    | "setActivityClipThreshold"
    | "setInputGainBoost"
    | "setPostUtteranceRefine"
    | "setPhraseBiasTerms"
    | "setCloudMode"
    | "setCloudOptIn"
    | "setReliabilityMode"
  >,
) => {
  setters.setModelProfile(runtime.modelProfile || "distil-large-v3");
  setters.setPerformanceProfile(runtime.performanceProfile || "whisper_balanced_english");
  setters.setDictationMode((runtime.dictationMode || "conversation") as DictationMode);
  setters.setToggleShortcut(runtime.toggleShortcut || "Ctrl+Shift+Space");
  setters.setOrtEp(runtime.ortEp || "auto");
  setters.setOrtIntraThreads(runtime.ortIntraThreads ?? 0);
  setters.setOrtInterThreads(runtime.ortInterThreads ?? 0);
  setters.setOrtParallel(runtime.ortParallel ?? true);
  setters.setLanguageHint(runtime.languageHint || "english");
  setters.setPillVisualizerSensitivity(runtime.pillVisualizerSensitivity || 10);
  setters.setActivitySensitivity(runtime.activitySensitivity || 4.2);
  setters.setActivityNoiseGate(runtime.activityNoiseGate ?? 0.0015);
  setters.setActivityClipThreshold(runtime.activityClipThreshold ?? 0.32);
  setters.setInputGainBoost(runtime.inputGainBoost || 1);
  setters.setPostUtteranceRefine(runtime.postUtteranceRefine ?? false);
  setters.setPhraseBiasTerms((runtime.phraseBiasTerms || []).join("\n"));
  setters.setCloudMode((runtime.cloudMode || "local_only") as CloudMode);
  setters.setCloudOptIn(runtime.cloudOptIn);
  setters.setReliabilityMode(runtime.reliabilityMode ?? true);
};

export function useGuidedTune(options: UseGuidedTuneOptions) {
  const {
    selectedDeviceName,
    isListening,
    startEngine,
    stopEngine,
    latestRmsRef,
    applyRuntime,
    setRuntimeMsg,
    setCalibrationMsg,
    setModelRecommendation,
    showOnboarding,
    setShowOnboarding,
    ...runtimeSetters
  } = options;

  const [isCalibrating, setIsCalibrating] = useState(false);
  const [isBenchmarkTuning, setIsBenchmarkTuning] = useState(false);
  const [guidedTuneStepLabel, setGuidedTuneStepLabel] = useState<string | null>(null);
  const [guidedTuneInstruction, setGuidedTuneInstruction] = useState<string | null>(null);
  const [guidedTuneSentence, setGuidedTuneSentence] = useState<string | null>(null);
  const [guidedTuneProgressPct, setGuidedTuneProgressPct] = useState(0);
  const [guidedTuneCompletedForDevice, setGuidedTuneCompletedForDevice] = useState(false);

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
  }, [latestRmsRef]);

  const sleep = useCallback((durationMs: number) => {
    return new Promise<void>((resolve) => {
      window.setTimeout(resolve, durationMs);
    });
  }, []);

  useEffect(() => {
    if (!selectedDeviceName) return;
    const all = loadCalibrationProfiles();
    const profile = all[selectedDeviceName];
    setGuidedTuneCompletedForDevice((profile?.guidedTuneVersion ?? 0) >= GUIDED_TUNE_VERSION);
    if (!profile) return;
    runtimeSetters.setPillVisualizerSensitivity(profile.pillVisualizerSensitivity);
    runtimeSetters.setActivitySensitivity(profile.activitySensitivity);
    runtimeSetters.setActivityNoiseGate(profile.activityNoiseGate);
    runtimeSetters.setActivityClipThreshold(profile.activityClipThreshold);
    runtimeSetters.setInputGainBoost(profile.inputGainBoost);
    void applyRuntime({
      pillVisualizerSensitivity: profile.pillVisualizerSensitivity,
      activitySensitivity: profile.activitySensitivity,
      activityNoiseGate: profile.activityNoiseGate,
      activityClipThreshold: profile.activityClipThreshold,
      inputGainBoost: profile.inputGainBoost,
    });
  }, [applyRuntime, runtimeSetters, selectedDeviceName]);

  const completeOnboarding = useCallback(async () => {
    if (!guidedTuneCompletedForDevice) {
      setCalibrationMsg("Run the guided voice tune before finishing setup. It takes about 30 seconds.");
      return;
    }
    const ok = await applyRuntime({ onboardingCompleted: true });
    if (!ok) return;
    setShowOnboarding(false);
    setRuntimeMsg("Onboarding complete.");
  }, [applyRuntime, guidedTuneCompletedForDevice, setCalibrationMsg, setRuntimeMsg, setShowOnboarding]);

  const runMicCalibration = useCallback(async () => {
    if (isCalibrating) return;
    setIsCalibrating(true);
    setGuidedTuneProgressPct(0);
    setGuidedTuneStepLabel("Preparing");
    setGuidedTuneInstruction("Starting guided voice tune...");
    setGuidedTuneSentence(null);
    setCalibrationMsg("Guided voice tune: getting ready...");
    const startedByCalibration = !isListening;
    try {
      if (!isListening) {
        await startEngine(selectedDeviceName);
        await sleep(500);
      }
      const ambientSamples: number[] = [];
      const whisperSamples: number[] = [];
      const normalSamples: number[] = [];
      let elapsedMs = 0;

      for (const stage of GUIDED_TUNE_STAGES) {
        if (stage.prepMs) {
          setGuidedTuneStepLabel(stage.label);
          setGuidedTuneInstruction(stage.instruction);
          setGuidedTuneSentence(stage.sentence ?? null);
          setCalibrationMsg(`${stage.label}: ${stage.instruction}`);
          setGuidedTuneProgressPct(Math.round((elapsedMs / GUIDED_TUNE_TOTAL_MS) * 100));
          await sleep(stage.prepMs);
          elapsedMs += stage.prepMs;
        }

        setGuidedTuneStepLabel(stage.label);
        setGuidedTuneInstruction(stage.instruction);
        setGuidedTuneSentence(stage.sentence ?? null);
        setCalibrationMsg(
          stage.sentence ? `${stage.label}: say "${stage.sentence}"` : `${stage.label}: ${stage.instruction}`,
        );
        setGuidedTuneProgressPct(Math.round((elapsedMs / GUIDED_TUNE_TOTAL_MS) * 100));
        const stageSamples = await collectRmsSamples(stage.captureMs);
        elapsedMs += stage.captureMs;

        if (stage.mode === "ambient") ambientSamples.push(...stageSamples);
        if (stage.mode === "whisper") whisperSamples.push(...stageSamples);
        if (stage.mode === "normal") normalSamples.push(...stageSamples);

        setGuidedTuneProgressPct(Math.round((elapsedMs / GUIDED_TUNE_TOTAL_MS) * 100));
      }

      setGuidedTuneStepLabel("Applying Tune");
      setGuidedTuneInstruction("Analyzing your samples and updating dictation tuning.");
      setGuidedTuneSentence(null);
      setCalibrationMsg("Guided voice tune: analyzing samples...");

      const ambientP90 = percentile(ambientSamples, 0.9);
      const whisperP70 = percentile(whisperSamples, 0.7);
      const normalP80 = percentile(normalSamples, 0.8);
      const perf = await getPerfSnapshot();
      const finalizeP95 = perf.finalizeMs?.p95Ms ?? 0;
      const fallbackRate = perf.diagnostics.finalSegmentsSeen
        ? (perf.diagnostics.fallbackStubTyped / perf.diagnostics.finalSegmentsSeen) * 100
        : 0;

      const tuned = await runBenchmarkAutoTune(ambientP90, whisperP70, normalP80, finalizeP95, fallbackRate);
      applyRuntimeSettings(tuned.runtimeSettings, runtimeSetters);

      if (selectedDeviceName) {
        saveCalibrationProfile(selectedDeviceName, {
          pillVisualizerSensitivity: tuned.runtimeSettings.pillVisualizerSensitivity || 10,
          activitySensitivity: tuned.runtimeSettings.activitySensitivity || 4.2,
          activityNoiseGate: tuned.runtimeSettings.activityNoiseGate ?? 0.0015,
          activityClipThreshold: tuned.runtimeSettings.activityClipThreshold ?? 0.32,
          inputGainBoost: tuned.runtimeSettings.inputGainBoost || 1,
          guidedTuneVersion: GUIDED_TUNE_VERSION,
          calibratedAt: new Date().toISOString(),
        });
      }

      setGuidedTuneCompletedForDevice(true);
      setGuidedTuneProgressPct(100);
      const envLabel = ambientP90 > 0.01 ? "noisy room" : "steady room";
      setCalibrationMsg(
        `Guided voice tune complete for ${envLabel}. Whisper and normal speech thresholds were updated in about 30 seconds.`,
      );
      setRuntimeMsg(tuned.summary);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setCalibrationMsg(`Guided voice tune failed: ${msg}`);
    } finally {
      if (startedByCalibration) {
        await stopEngine().catch(() => undefined);
      }
      setGuidedTuneStepLabel(null);
      setGuidedTuneInstruction(null);
      setGuidedTuneSentence(null);
      setIsCalibrating(false);
    }
  }, [
    collectRmsSamples,
    isCalibrating,
    isListening,
    runtimeSetters,
    selectedDeviceName,
    setCalibrationMsg,
    setRuntimeMsg,
    sleep,
    startEngine,
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

      const tuned = await runBenchmarkAutoTune(ambientP90, whisperP70, normalP80, finalizeP95, fallbackRate);
      applyRuntimeSettings(tuned.runtimeSettings, runtimeSetters);
      setRuntimeMsg(tuned.summary);
      setCalibrationMsg(
        `Benchmark tune complete. p95 ${Math.round(tuned.measuredFinalizeP95Ms)}ms · fallback ${tuned.measuredFallbackRatePct.toFixed(1)}%.`,
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
    isBenchmarkTuning,
    isListening,
    runtimeSetters,
    selectedDeviceName,
    setCalibrationMsg,
    setModelRecommendation,
    setRuntimeMsg,
    startEngine,
    stopEngine,
  ]);

  return {
    showOnboarding,
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
  };
}
