"use client";

import { GuidedTuneCard } from "@/components/settings/GuidedTuneCard";

type CloudMode = "local_only" | "hybrid" | "cloud_preferred";
type DictationMode = "conversation" | "coding" | "command";

type SelectOption<T extends string> = {
  value: T;
  label: string;
};

type DictationModeOption = SelectOption<DictationMode> & {
  hint: string;
};

type OnboardingModalProps = {
  visible: boolean;
  toggleShortcut: string;
  modelProfile: string;
  performanceProfile: string;
  dictationMode: DictationMode;
  cloudMode: CloudMode;
  openAiApiKeyInput: string;
  hasOpenAiApiKey: boolean;
  reliabilityMode: boolean;
  onboardingCompleted: boolean;
  guidedTuneCompletedForDevice: boolean;
  guidedTuneProgressPct: number;
  guidedTuneStepLabel: string;
  guidedTuneInstruction: string;
  guidedTuneSentence: string | null;
  runtimeMsg: string | null;
  calibrationMsg: string | null;
  isCalibrating: boolean;
  isBenchmarkTuning: boolean;
  modelProfileOptions: SelectOption<string>[];
  performanceProfileOptions: SelectOption<string>[];
  dictationModeOptions: DictationModeOption[];
  cloudModeOptions: Array<SelectOption<CloudMode>>;
  recommendedModelLabel: string | null;
  modelRecommendationReason: string | null;
  onToggleShortcutChange: (value: string) => void;
  onModelProfileChange: (value: string) => void;
  onApplyRecommendedModel: () => void;
  onPerformanceProfileChange: (value: string) => void;
  onDictationModeChange: (value: DictationMode) => void;
  onCloudModeChange: (value: CloudMode) => void;
  onOpenAiApiKeyInputChange: (value: string) => void;
  onReliabilityModeChange: (value: boolean) => void;
  onRunMicCalibration: () => void | Promise<void>;
  onRunBenchmarkTune: () => void | Promise<void>;
  onCompleteOnboarding: () => void | Promise<void>;
  onClose: () => void;
};

export function OnboardingModal({
  visible,
  toggleShortcut,
  modelProfile,
  performanceProfile,
  dictationMode,
  cloudMode,
  openAiApiKeyInput,
  hasOpenAiApiKey,
  reliabilityMode,
  onboardingCompleted,
  guidedTuneCompletedForDevice,
  guidedTuneProgressPct,
  guidedTuneStepLabel,
  guidedTuneInstruction,
  guidedTuneSentence,
  runtimeMsg,
  calibrationMsg,
  isCalibrating,
  isBenchmarkTuning,
  modelProfileOptions,
  performanceProfileOptions,
  dictationModeOptions,
  cloudModeOptions,
  recommendedModelLabel,
  modelRecommendationReason,
  onToggleShortcutChange,
  onModelProfileChange,
  onApplyRecommendedModel,
  onPerformanceProfileChange,
  onDictationModeChange,
  onCloudModeChange,
  onOpenAiApiKeyInputChange,
  onReliabilityModeChange,
  onRunMicCalibration,
  onRunBenchmarkTune,
  onCompleteOnboarding,
  onClose,
}: OnboardingModalProps) {
  if (!visible) return null;

  return (
    <div className="onboarding-backdrop" data-no-drag>
      <section className="onboarding-modal">
        <span className="settings-kicker">First Launch</span>
        <h2>Welcome to Dictum</h2>
        <p>Set your keybind, transcription defaults, and run the guided 30-second voice tune before starting.</p>

        <div className="settings-fields">
          <label className="settings-field">
            <span>Toggle Shortcut</span>
            <input
              className="settings-input"
              value={toggleShortcut}
              onChange={(e) => onToggleShortcutChange(e.target.value)}
              placeholder="Ctrl+Shift+Space"
            />
          </label>
          <label className="settings-field">
            <span>Model</span>
            <select className="settings-input" value={modelProfile} onChange={(e) => onModelProfileChange(e.target.value)}>
              {modelProfileOptions.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
          </label>
          {recommendedModelLabel && modelRecommendationReason && (
            <div className="settings-inline-actions">
              <button className="action-btn" onClick={onApplyRecommendedModel} type="button">
                Use Recommended ({recommendedModelLabel})
              </button>
              <span className="settings-note">{modelRecommendationReason}</span>
            </div>
          )}
          <label className="settings-field">
            <span>Performance</span>
            <select
              className="settings-input"
              value={performanceProfile}
              onChange={(e) => onPerformanceProfileChange(e.target.value)}
            >
              {performanceProfileOptions.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
          </label>
          <label className="settings-field">
            <span>Dictation Mode</span>
            <select
              className="settings-input"
              value={dictationMode}
              onChange={(e) => onDictationModeChange(e.target.value as DictationMode)}
            >
              {dictationModeOptions.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
          </label>
          <label className="settings-field">
            <span>Cloud Mode</span>
            <select className="settings-input" value={cloudMode} onChange={(e) => onCloudModeChange(e.target.value as CloudMode)}>
              {cloudModeOptions.map((opt) => (
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
            onChange={(e) => onOpenAiApiKeyInputChange(e.target.value)}
            placeholder={hasOpenAiApiKey ? "Saved locally. Enter new key to replace." : "sk-proj-..."}
            autoComplete="off"
          />
        </label>

        <label className="settings-switch">
          <input
            type="checkbox"
            checked={reliabilityMode}
            onChange={(e) => onReliabilityModeChange(e.target.checked)}
          />
          <span>Enable reliability mode (recommended)</span>
        </label>

        <p className="settings-note">
          {dictationModeOptions.find((opt) => opt.value === dictationMode)?.hint}
        </p>

        <GuidedTuneCard
          kicker="Required Step"
          title="Guided Voice Tune"
          readyLabel="Complete"
          pendingLabel="Pending"
          completed={guidedTuneCompletedForDevice}
          note="Read two short sentences at normal volume and two in a whisper. Dictum uses that sample to tune gain, speech sensitivity, and latency defaults for this microphone."
          progressPct={guidedTuneProgressPct}
          stepLabel={guidedTuneStepLabel}
          instruction={guidedTuneInstruction}
          sentence={guidedTuneSentence}
          className="onboarding-guided-tune"
        />

        <div className="settings-inline-actions">
          <button className="action-btn" onClick={() => void onRunMicCalibration()} disabled={isCalibrating} type="button">
            {isCalibrating ? "Running guided tune..." : "Run Guided Voice Tune"}
          </button>
          <button className="action-btn" onClick={() => void onRunBenchmarkTune()} disabled={isBenchmarkTuning} type="button">
            {isBenchmarkTuning ? "Benchmarking..." : "Advanced Benchmark Tune"}
          </button>
          <button
            className="action-btn settings-save-btn"
            onClick={() => void onCompleteOnboarding()}
            disabled={!guidedTuneCompletedForDevice || isCalibrating}
            type="button"
          >
            Finish Setup
          </button>
          {onboardingCompleted && (
            <button className="action-btn" onClick={onClose} type="button">
              Close
            </button>
          )}
        </div>

        <p className="settings-note">
          {guidedTuneCompletedForDevice
            ? runtimeMsg ?? "You can re-open onboarding anytime from Settings."
            : calibrationMsg ?? "Run the guided voice tune to unlock Finish Setup."}
        </p>
      </section>
    </div>
  );
}
