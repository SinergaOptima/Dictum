"use client";

import { GuidedTuneCard } from "@/components/settings/GuidedTuneCard";

type CloudMode = "local_only" | "hybrid" | "cloud_preferred";

type CloudModeOption = {
  value: CloudMode;
  label: string;
  hint: string;
};

type PrivacySectionProps = {
  openAiApiKeyInput: string;
  hasOpenAiApiKey: boolean;
  cloudMode: CloudMode;
  cloudModeOptions: CloudModeOption[];
  historyEnabled: boolean;
  guidedTuneCompletedForDevice: boolean;
  guidedTuneProgressPct: number;
  guidedTuneStepLabel: string;
  guidedTuneInstruction: string;
  guidedTuneSentence: string | null;
  runtimeMsg: string | null;
  calibrationMsg: string | null;
  onOpenAiApiKeyInputChange: (value: string) => void;
  onCloudModeChange: (value: CloudMode) => void;
  onHistoryEnabledChange: (value: boolean) => void;
  onClearSavedKey: () => void | Promise<void> | Promise<boolean>;
};

export function PrivacySection({
  openAiApiKeyInput,
  hasOpenAiApiKey,
  cloudMode,
  cloudModeOptions,
  historyEnabled,
  guidedTuneCompletedForDevice,
  guidedTuneProgressPct,
  guidedTuneStepLabel,
  guidedTuneInstruction,
  guidedTuneSentence,
  runtimeMsg,
  calibrationMsg,
  onOpenAiApiKeyInputChange,
  onCloudModeChange,
  onHistoryEnabledChange,
  onClearSavedKey,
}: PrivacySectionProps) {
  return (
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
          onChange={(e) => onOpenAiApiKeyInputChange(e.target.value)}
          placeholder={hasOpenAiApiKey ? "Saved locally. Enter new key to replace." : "sk-proj-..."}
          autoComplete="off"
        />
      </label>
      <div className="settings-inline-actions">
        <button
          className="action-btn"
          disabled={!hasOpenAiApiKey}
          onClick={() => void onClearSavedKey()}
          type="button"
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
          onChange={(e) => onCloudModeChange(e.target.value as CloudMode)}
        >
          {cloudModeOptions.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </label>
      <p className="settings-note">
        {cloudModeOptions.find((opt) => opt.value === cloudMode)?.hint}
      </p>
      <label className="settings-switch">
        <input
          type="checkbox"
          checked={historyEnabled}
          onChange={(e) => onHistoryEnabledChange(e.target.checked)}
        />
        <span>Save local history</span>
      </label>
      <GuidedTuneCard
        kicker="0.1.8 Tune"
        title="Guided Voice Tune"
        readyLabel="Ready"
        pendingLabel="Needs run"
        completed={guidedTuneCompletedForDevice}
        note="A scripted 30-second calibration that listens to room tone, normal speech, and whisper speech."
        progressPct={guidedTuneProgressPct}
        stepLabel={guidedTuneStepLabel}
        instruction={guidedTuneInstruction}
        sentence={guidedTuneSentence}
      />
      <p className="settings-note">
        {runtimeMsg ?? "Cloud fallback requires an OpenAI API key."}
      </p>
      <p className="settings-note">
        {calibrationMsg ?? "Run the guided voice tune after changing microphones or recording environments."}
      </p>
    </article>
  );
}
