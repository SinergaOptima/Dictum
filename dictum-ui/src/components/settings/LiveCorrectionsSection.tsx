"use client";

import type { ActiveAppContext, AppProfile, LearnedCorrection } from "@shared/ipc_types";
import type {
  CorrectionFilterScope,
  CorrectionScope,
  CorrectionSort,
} from "@/hooks/useCorrectionsManager";

type LiveCorrectionsSectionProps = {
  correctionHeardInput: string;
  correctionFixedInput: string;
  correctionsCopied: "idle" | "done" | "error";
  correctionFilter: string;
  correctionsImportText: string;
  learnedCorrections: LearnedCorrection[];
  filteredLearnedCorrections: LearnedCorrection[];
  activeAppContext: ActiveAppContext | null;
  appProfiles: AppProfile[];
  currentDictationMode: "conversation" | "coding" | "command";
  correctionScope: CorrectionScope;
  correctionFilterScope: CorrectionFilterScope;
  correctionSort: CorrectionSort;
  editingCorrection: LearnedCorrection | null;
  onCorrectionHeardInputChange: (value: string) => void;
  onCorrectionFixedInputChange: (value: string) => void;
  onCorrectionFilterChange: (value: string) => void;
  onCorrectionsImportTextChange: (value: string) => void;
  onCorrectionScopeChange: (value: CorrectionScope) => void;
  onCorrectionFilterScopeChange: (value: CorrectionFilterScope) => void;
  onCorrectionSortChange: (value: CorrectionSort) => void;
  onLearnCorrection: () => void | Promise<void>;
  onCopyCorrections: () => void | Promise<void>;
  onImportCorrections: () => void | Promise<void>;
  onDeleteCorrection: (rule: LearnedCorrection) => void | Promise<void>;
  onStartEditingCorrection: (rule: LearnedCorrection) => void | Promise<void>;
  onCancelEditingCorrection: () => void | Promise<void>;
};

function getRuleScope(rule: LearnedCorrection): CorrectionScope {
  if (rule.appProfileAffinity) return "profile";
  if (rule.modeAffinity) return "mode";
  return "global";
}

export function LiveCorrectionsSection({
  correctionHeardInput,
  correctionFixedInput,
  correctionsCopied,
  correctionFilter,
  correctionsImportText,
  learnedCorrections,
  filteredLearnedCorrections,
  activeAppContext,
  appProfiles,
  currentDictationMode,
  correctionScope,
  correctionFilterScope,
  correctionSort,
  editingCorrection,
  onCorrectionHeardInputChange,
  onCorrectionFixedInputChange,
  onCorrectionFilterChange,
  onCorrectionsImportTextChange,
  onCorrectionScopeChange,
  onCorrectionFilterScopeChange,
  onCorrectionSortChange,
  onLearnCorrection,
  onCopyCorrections,
  onImportCorrections,
  onDeleteCorrection,
  onStartEditingCorrection,
  onCancelEditingCorrection,
}: LiveCorrectionsSectionProps) {
  const profileNameById = new Map(appProfiles.map((profile) => [profile.id, profile.name]));
  const activeMode = activeAppContext?.dictationMode || currentDictationMode;
  const activeProfileId = activeAppContext?.matchedProfileId || null;

  return (
    <article className="settings-card">
      <div className="settings-card-header">
        <h3>Live Corrections</h3>
        <p>Teach Dictum how to fix common mishears, edit scoped rules, and keep large rule sets manageable.</p>
      </div>
      <div className="settings-fields">
        <label className="settings-field">
          <span>Heard</span>
          <input
            className="settings-input"
            value={correctionHeardInput}
            onChange={(e) => onCorrectionHeardInputChange(e.target.value)}
            placeholder="ex: ladder labs"
          />
        </label>
        <label className="settings-field">
          <span>Corrected</span>
          <input
            className="settings-input"
            value={correctionFixedInput}
            onChange={(e) => onCorrectionFixedInputChange(e.target.value)}
            placeholder="ex: Lattice Labs"
          />
        </label>
      </div>
      <div className="settings-inline-actions">
        <button className="action-btn" onClick={() => void onLearnCorrection()} type="button">
          {editingCorrection ? "Save Changes" : "Learn Correction"}
        </button>
        {editingCorrection && (
          <button className="action-btn secondary" onClick={() => void onCancelEditingCorrection()} type="button">
            Cancel Edit
          </button>
        )}
        <button className="action-btn" onClick={() => void onCopyCorrections()} type="button">
          {correctionsCopied === "done"
            ? "Copied"
            : correctionsCopied === "error"
              ? "Copy Failed"
              : "Copy JSON"}
        </button>
        <span className="settings-note">{learnedCorrections.length} learned rules</span>
      </div>
      <div className="settings-field">
        <span>Correction Scope</span>
        <div className="settings-chip-row">
          <button
            className={`settings-chip-btn ${correctionScope === "global" ? "is-active" : ""}`}
            onClick={() => onCorrectionScopeChange("global")}
            type="button"
          >
            Global
          </button>
          <button
            className={`settings-chip-btn ${correctionScope === "mode" ? "is-active" : ""}`}
            onClick={() => onCorrectionScopeChange("mode")}
            type="button"
          >
            Mode Only
          </button>
          <button
            className={`settings-chip-btn ${correctionScope === "profile" ? "is-active" : ""}`}
            onClick={() => onCorrectionScopeChange("profile")}
            type="button"
            disabled={!activeAppContext?.matchedProfileId}
            title={
              activeAppContext?.matchedProfileName
                ? `Save only for ${activeAppContext.matchedProfileName}`
                : "No active app profile is matched right now"
            }
          >
            Active Profile
          </button>
        </div>
      </div>
      <p className="settings-note">
        {correctionScope === "global"
          ? "This correction applies across all apps and modes."
          : correctionScope === "mode"
            ? `This correction applies only in ${currentDictationMode} mode.`
            : activeAppContext?.matchedProfileName
              ? `This correction applies only to ${activeAppContext.matchedProfileName}.`
              : `No active app profile right now, so this will fall back to ${activeMode} mode.`}
      </p>
      {!activeAppContext?.matchedProfileId && (
        <p className="settings-note">
          Profile-scoped correction saves are disabled until Dictum detects a foreground app with a matching saved profile.
        </p>
      )}
      <div className="settings-fields">
        <label className="settings-field">
          <span>Browse Rules</span>
          <div className="settings-chip-row">
            {(["current", "all", "global", "mode", "profile"] as const).map((value) => (
              <button
                key={value}
                className={`settings-chip-btn ${correctionFilterScope === value ? "is-active" : ""}`}
                onClick={() => onCorrectionFilterScopeChange(value)}
                type="button"
              >
                {value === "current"
                  ? "Current Context"
                  : value === "all"
                    ? "All Rules"
                    : value === "mode"
                      ? "Mode Rules"
                      : value === "profile"
                        ? "Profile Rules"
                        : "Global Rules"}
              </button>
            ))}
          </div>
        </label>
        <label className="settings-field">
          <span>Sort</span>
          <select
            className="settings-input"
            value={correctionSort}
            onChange={(e) => onCorrectionSortChange(e.target.value as CorrectionSort)}
          >
            <option value="best">Best Match</option>
            <option value="recent">Recently Used</option>
            <option value="hits">Most Used</option>
            <option value="heard">Alphabetical</option>
          </select>
        </label>
      </div>
      <label className="settings-field">
        <span>Filter Rules</span>
        <input
          className="settings-input"
          value={correctionFilter}
          onChange={(e) => onCorrectionFilterChange(e.target.value)}
          placeholder="Search heard text, corrected text, mode, or profile"
        />
      </label>
      <label className="settings-field">
        <span>Import Corrections JSON</span>
        <textarea
          className="settings-input settings-textarea"
          value={correctionsImportText}
          onChange={(e) => onCorrectionsImportTextChange(e.target.value)}
          placeholder='[{"heard":"ladder labs","corrected":"Lattice Labs","hits":1}]'
          rows={4}
        />
      </label>
      <div className="settings-inline-actions">
        <button className="action-btn" onClick={() => void onImportCorrections()} type="button">
          Import Corrections
        </button>
      </div>
      <div className="panel-list">
        {filteredLearnedCorrections.slice(0, 20).map((rule) => {
          const ruleScope = getRuleScope(rule);
          const profileName = rule.appProfileAffinity
            ? profileNameById.get(rule.appProfileAffinity) || rule.appProfileAffinity
            : null;
          const matchesCurrentContext = rule.appProfileAffinity
            ? rule.appProfileAffinity === activeProfileId
            : rule.modeAffinity
              ? rule.modeAffinity === activeMode
              : true;
          const isEditing =
            editingCorrection?.heard === rule.heard &&
            editingCorrection.corrected === rule.corrected &&
            editingCorrection.modeAffinity === rule.modeAffinity &&
            editingCorrection.appProfileAffinity === rule.appProfileAffinity;

          return (
            <article
              key={`${rule.heard}:${rule.corrected}:${rule.modeAffinity || "any"}:${rule.appProfileAffinity || "all"}`}
              className={`panel-card ${isEditing ? "is-editing" : ""} ${matchesCurrentContext ? "is-active" : ""}`}
            >
              <div className="panel-meta">
                <span>{rule.heard}</span>
                <span>→</span>
                <span>{rule.corrected}</span>
                <span>hits {rule.hits}</span>
                <span>{ruleScope}</span>
                {rule.modeAffinity && <span>{rule.modeAffinity}</span>}
                {profileName && <span>{profileName}</span>}
                <span>{matchesCurrentContext ? "matches current context" : "out of current context"}</span>
              </div>
              <p>
                {ruleScope === "profile"
                  ? `Profile ${profileName}`
                  : ruleScope === "mode"
                    ? `Mode ${rule.modeAffinity}`
                    : "All apps and modes"}
                {rule.lastUsedAt ? ` · used ${new Date(rule.lastUsedAt).toLocaleString()}` : " · not used live yet"}
              </p>
              <div className="settings-inline-actions">
                <button className="action-btn" onClick={() => void onStartEditingCorrection(rule)} type="button">
                  Edit
                </button>
                <button className="action-btn secondary" onClick={() => void onDeleteCorrection(rule)} type="button">
                  Remove
                </button>
              </div>
            </article>
          );
        })}
        {filteredLearnedCorrections.length === 0 && (
          <p className="settings-note">
            {learnedCorrections.length === 0
              ? "No learned corrections yet."
              : "No learned corrections match the current browsing filters."}
          </p>
        )}
      </div>
    </article>
  );
}
