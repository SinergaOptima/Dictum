"use client";

import type { ActiveAppContext, AppProfile } from "@shared/ipc_types";

type DictationMode = AppProfile["dictationMode"];

type DictationModeOption = {
  value: DictationMode;
  label: string;
  hint: string;
};

type AppProfilePreset = {
  name: string;
  appMatch: string;
  dictationMode: DictationMode;
  phraseBiasTerms: string[];
  postUtteranceRefine: boolean;
};

type PerAppProfilesSectionProps = {
  profiles: AppProfile[];
  activeAppContext: ActiveAppContext | null;
  globalDictationMode: DictationMode;
  globalPhraseBiasCount: number;
  profileName: string;
  profileMatch: string;
  profileMode: DictationMode;
  profileBiasTerms: string;
  profileRefine: boolean;
  editingProfileId: string | null;
  importText: string;
  copiedState: "idle" | "done" | "error";
  modeOptions: DictationModeOption[];
  presets: AppProfilePreset[];
  onNameChange: (value: string) => void;
  onMatchChange: (value: string) => void;
  onModeChange: (value: DictationMode) => void;
  onBiasTermsChange: (value: string) => void;
  onRefineChange: (value: boolean) => void;
  onImportTextChange: (value: string) => void;
  onSave: () => void | Promise<void>;
  onCopy: () => void | Promise<void>;
  onImport: () => void | Promise<void>;
  onEdit: (profile: AppProfile) => void;
  onDelete: (id: string, name: string) => void | Promise<void>;
  onCancelEdit: () => void;
  onApplyPreset: (preset: AppProfilePreset) => void;
};

export function PerAppProfilesSection({
  profiles,
  activeAppContext,
  globalDictationMode,
  globalPhraseBiasCount,
  profileName,
  profileMatch,
  profileMode,
  profileBiasTerms,
  profileRefine,
  editingProfileId,
  importText,
  copiedState,
  modeOptions,
  presets,
  onNameChange,
  onMatchChange,
  onModeChange,
  onBiasTermsChange,
  onRefineChange,
  onImportTextChange,
  onSave,
  onCopy,
  onImport,
  onEdit,
  onDelete,
  onCancelEdit,
  onApplyPreset,
}: PerAppProfilesSectionProps) {
  return (
    <article className="settings-card">
      <div className="settings-card-header">
        <h3>Per-App Profiles</h3>
        <p>Automatically switch dictation mode when the foreground app matches an executable name.</p>
      </div>
      <div className="settings-inline-stats">
        <span>Foreground {activeAppContext?.foregroundApp || "unknown"}</span>
        <span>Active profile {activeAppContext?.matchedProfileName || "global default"}</span>
        <span>Mode {activeAppContext?.dictationMode || globalDictationMode}</span>
        <span>Bias terms {activeAppContext?.phraseBiasTermCount ?? globalPhraseBiasCount}</span>
        <span>{activeAppContext?.postUtteranceRefine ? "Refine on" : "Refine off"}</span>
      </div>
      <div className="settings-chip-row">
        {presets.map((preset) => (
          <button
            key={preset.appMatch}
            className="settings-chip-btn"
            onClick={() => onApplyPreset(preset)}
            type="button"
          >
            {preset.name}
          </button>
        ))}
      </div>
      <p className="settings-note">
        Presets fill the editor with a starting mode and vocabulary. You can paste either a full Windows path or an executable name, and Dictum will normalize it to `cursor.exe` or `slack.exe`.
      </p>
      <div className="settings-fields">
        <label className="settings-field">
          <span>Profile Name</span>
          <input
            className="settings-input"
            value={profileName}
            onChange={(e) => onNameChange(e.target.value)}
            placeholder="Cursor Coding"
          />
        </label>
        <label className="settings-field">
          <span>App Match</span>
          <input
            className="settings-input"
            value={profileMatch}
            onChange={(e) => onMatchChange(e.target.value)}
            placeholder="cursor.exe"
          />
        </label>
        <label className="settings-field">
          <span>Mode Override</span>
          <select
            className="settings-input"
            value={profileMode}
            onChange={(e) => onModeChange(e.target.value as DictationMode)}
          >
            {modeOptions.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </label>
        <label className="settings-field">
          <span>Bias Terms</span>
          <textarea
            className="settings-input settings-textarea"
            value={profileBiasTerms}
            onChange={(e) => onBiasTermsChange(e.target.value)}
            placeholder={"One term per line.\nTypeScript\nPostgreSQL"}
            rows={4}
          />
        </label>
      </div>
      <label className="settings-switch">
        <input
          type="checkbox"
          checked={profileRefine}
          onChange={(e) => onRefineChange(e.target.checked)}
        />
        <span>Enable post-utterance refinement for this app profile</span>
      </label>
      <div className="settings-inline-actions">
        <button className="action-btn" onClick={() => void onSave()} type="button">
          {editingProfileId ? "Update App Profile" : "Save App Profile"}
        </button>
        {editingProfileId && (
          <button className="action-btn" onClick={onCancelEdit} type="button">
            Cancel Edit
          </button>
        )}
        <button className="action-btn" onClick={() => void onCopy()} type="button">
          {copiedState === "done"
            ? "Copied"
            : copiedState === "error"
              ? "Copy Failed"
              : "Copy JSON"}
        </button>
        <span className="settings-note">
          Matches are stored as executable names like `cursor.exe`, `code.exe`, or `windowsterminal.exe`.
        </span>
      </div>
      <label className="settings-field">
        <span>Import Profiles JSON</span>
        <textarea
          className="settings-input settings-textarea"
          value={importText}
          onChange={(e) => onImportTextChange(e.target.value)}
          placeholder='[{"name":"Cursor Coding","appMatch":"cursor.exe","dictationMode":"coding","phraseBiasTerms":["TypeScript"],"postUtteranceRefine":true,"enabled":true}]'
          rows={4}
        />
      </label>
      <div className="settings-inline-actions">
        <button className="action-btn" onClick={() => void onImport()} type="button">
          Import Profiles
        </button>
      </div>
      <div className="panel-list">
        {profiles.length === 0 ? (
          <p className="settings-note">No app-specific profiles yet.</p>
        ) : (
          profiles.map((profile) => {
            const isActive = profile.id === activeAppContext?.matchedProfileId;
            const isEditing = profile.id === editingProfileId;
            return (
              <article
                key={profile.id}
                className={`panel-card ${isActive ? "is-active" : ""} ${isEditing ? "is-editing" : ""}`.trim()}
              >
                <div className="panel-meta">
                  <span>{profile.name}</span>
                  <span>{profile.appMatch}</span>
                  <span>{profile.dictationMode}</span>
                  {profile.postUtteranceRefine && <span>refine</span>}
                  {isActive && <span>active now</span>}
                  {isEditing && <span>editing</span>}
                </div>
                {profile.phraseBiasTerms.length > 0 && <p>{profile.phraseBiasTerms.join(", ")}</p>}
                <div className="context-menu">
                  <button className="context-action" onClick={() => onEdit(profile)} type="button">
                    Edit
                  </button>
                  <button
                    className="context-action danger"
                    onClick={() => void onDelete(profile.id, profile.name)}
                    type="button"
                  >
                    Delete
                  </button>
                </div>
              </article>
            );
          })
        )}
      </div>
    </article>
  );
}
