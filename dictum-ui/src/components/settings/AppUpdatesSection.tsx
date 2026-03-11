"use client";

import type { AppUpdateInfo } from "@shared/ipc_types";

type UpdateTelemetryEvent = {
  id: string;
  at: string;
  event: string;
  detail: string;
  source: "manual" | "startup-auto" | "idle-auto" | "system";
};

type AppUpdatesSectionProps = {
  updateAutoCheckEnabled: boolean;
  updateAutoInstallWhenIdle: boolean;
  updateRepoSlug: string;
  isCheckingUpdate: boolean;
  isInstallingUpdate: boolean;
  updateInfo: AppUpdateInfo | null;
  updateLogCopied: "idle" | "done" | "error";
  updateLastCheckedAt: string | null;
  updateSkipVersion: string | null;
  updateRemindUntilMs: number;
  currentAppVersion: string;
  updateTelemetry: UpdateTelemetryEvent[];
  onUpdateAutoCheckEnabledChange: (value: boolean) => void;
  onUpdateAutoInstallWhenIdleChange: (value: boolean) => void;
  onUpdateRepoSlugChange: (value: string) => void;
  onCheckForUpdates: () => void | Promise<void>;
  onInstallUpdate: () => void | Promise<void>;
  onRemindLater: () => void;
  onSkipThisVersion: () => void;
  onClearDeferrals: () => void;
  onExportUpdateTelemetry: () => void | Promise<void>;
};

export function AppUpdatesSection({
  updateAutoCheckEnabled,
  updateAutoInstallWhenIdle,
  updateRepoSlug,
  isCheckingUpdate,
  isInstallingUpdate,
  updateInfo,
  updateLogCopied,
  updateLastCheckedAt,
  updateSkipVersion,
  updateRemindUntilMs,
  currentAppVersion,
  updateTelemetry,
  onUpdateAutoCheckEnabledChange,
  onUpdateAutoInstallWhenIdleChange,
  onUpdateRepoSlugChange,
  onCheckForUpdates,
  onInstallUpdate,
  onRemindLater,
  onSkipThisVersion,
  onClearDeferrals,
  onExportUpdateTelemetry,
}: AppUpdatesSectionProps) {
  return (
    <article className="settings-card">
      <div className="settings-card-header">
        <h3>App Updates</h3>
        <p>Background startup checks, manual check, and installer launch from GitHub Releases.</p>
      </div>
      <label className="settings-switch">
        <input
          type="checkbox"
          checked={updateAutoCheckEnabled}
          onChange={(e) => onUpdateAutoCheckEnabledChange(e.target.checked)}
        />
        <span>Auto-check for updates on startup</span>
      </label>
      <label className="settings-switch">
        <input
          type="checkbox"
          checked={updateAutoInstallWhenIdle}
          onChange={(e) => onUpdateAutoInstallWhenIdleChange(e.target.checked)}
        />
        <span>Auto-install updates when idle (2+ min, not listening)</span>
      </label>
      <label className="settings-field">
        <span>Repository</span>
        <input
          className="settings-input"
          value={updateRepoSlug}
          onChange={(e) => onUpdateRepoSlugChange(e.target.value)}
          placeholder="owner/repo"
        />
      </label>
      <div className="settings-inline-actions">
        <button
          className="action-btn"
          onClick={() => void onCheckForUpdates()}
          disabled={isCheckingUpdate}
          type="button"
        >
          {isCheckingUpdate ? "Checking..." : "Check for updates"}
        </button>
        <button
          className="action-btn"
          onClick={() => void onInstallUpdate()}
          disabled={
            isInstallingUpdate ||
            !updateInfo?.hasUpdate ||
            !updateInfo?.assetDownloadUrl ||
            !updateInfo?.expectedInstallerSha256
          }
          type="button"
        >
          {isInstallingUpdate ? "Launching..." : "Install available update"}
        </button>
        <button className="action-btn" onClick={onRemindLater} disabled={!updateInfo?.hasUpdate} type="button">
          Remind Later
        </button>
        <button className="action-btn" onClick={onSkipThisVersion} disabled={!updateInfo?.hasUpdate} type="button">
          Skip This Version
        </button>
        <button className="action-btn" onClick={onClearDeferrals} type="button">
          Clear Skip/Snooze
        </button>
        {updateInfo?.htmlUrl && (
          <a className="action-btn" href={updateInfo.htmlUrl} target="_blank" rel="noreferrer">
            Open release page
          </a>
        )}
        <button className="action-btn" onClick={() => void onExportUpdateTelemetry()} type="button">
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
  );
}
