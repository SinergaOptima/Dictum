"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type { AppUpdateInfo } from "@shared/ipc_types";
import {
  checkForAppUpdate,
  downloadAndInstallAppUpdate,
  getAppVersion,
} from "@/lib/tauri";

type UpdateTelemetryEvent = {
  id: string;
  at: string;
  event: string;
  detail: string;
  source: "manual" | "startup-auto" | "idle-auto" | "system";
};

type UpdateCheckOptions = {
  silent?: boolean;
  ignoreDeferrals?: boolean;
  source?: "manual" | "startup-auto";
};

type UpdateInstallOptions = {
  autoExit?: boolean;
  source?: "manual" | "banner" | "idle-auto";
};

const DEFAULT_UPDATE_REPO = "sinergaoptima/dictum";
const LEGACY_UPDATE_REPOS = new Set(["latticelabs/dictum"]);
const UPDATE_REPO_STORAGE_KEY = "dictum-update-repo-v1";
const UPDATE_AUTO_CHECK_STORAGE_KEY = "dictum-update-auto-check-v1";
const UPDATE_SKIP_VERSION_STORAGE_KEY = "dictum-update-skip-version-v1";
const UPDATE_REMIND_UNTIL_STORAGE_KEY = "dictum-update-remind-until-v1";
const UPDATE_LAST_CHECKED_STORAGE_KEY = "dictum-update-last-checked-v1";
const UPDATE_AUTO_INSTALL_IDLE_STORAGE_KEY = "dictum-update-auto-install-idle-v1";
const UPDATE_TELEMETRY_STORAGE_KEY = "dictum-update-telemetry-v1";
const UPDATE_IDLE_INSTALL_GRACE_MS = 120_000;

type UseUpdateManagerOptions = {
  isListening: boolean;
  showOnboarding: boolean;
  setRuntimeMsg: (msg: string | null) => void;
};

export function useUpdateManager({
  isListening,
  showOnboarding,
  setRuntimeMsg,
}: UseUpdateManagerOptions) {
  const autoUpdateCheckedRef = useRef(false);
  const lastUserInteractionAtRef = useRef(Date.now());
  const autoInstallAttemptedForVersionRef = useRef<string | null>(null);

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
    getAppVersion()
      .then((version) => setCurrentAppVersion(version))
      .catch(() => setCurrentAppVersion("dev"));
  }, []);

  useEffect(() => {
    try {
      const savedRepo = localStorage.getItem(UPDATE_REPO_STORAGE_KEY);
      if (savedRepo && savedRepo.trim()) {
        const normalizedRepo = savedRepo.trim().toLowerCase();
        setUpdateRepoSlug(
          LEGACY_UPDATE_REPOS.has(normalizedRepo) ? DEFAULT_UPDATE_REPO : savedRepo.trim(),
        );
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
    } catch {}
  }, [updateRepoSlug]);

  useEffect(() => {
    try {
      localStorage.setItem(UPDATE_AUTO_CHECK_STORAGE_KEY, updateAutoCheckEnabled ? "1" : "0");
    } catch {}
  }, [updateAutoCheckEnabled]);

  useEffect(() => {
    try {
      if (updateSkipVersion && updateSkipVersion.trim()) {
        localStorage.setItem(UPDATE_SKIP_VERSION_STORAGE_KEY, updateSkipVersion.trim());
      } else {
        localStorage.removeItem(UPDATE_SKIP_VERSION_STORAGE_KEY);
      }
    } catch {}
  }, [updateSkipVersion]);

  useEffect(() => {
    try {
      if (updateRemindUntilMs > 0) {
        localStorage.setItem(UPDATE_REMIND_UNTIL_STORAGE_KEY, String(Math.floor(updateRemindUntilMs)));
      } else {
        localStorage.removeItem(UPDATE_REMIND_UNTIL_STORAGE_KEY);
      }
    } catch {}
  }, [updateRemindUntilMs]);

  useEffect(() => {
    try {
      if (updateLastCheckedAt && updateLastCheckedAt.trim()) {
        localStorage.setItem(UPDATE_LAST_CHECKED_STORAGE_KEY, updateLastCheckedAt);
      } else {
        localStorage.removeItem(UPDATE_LAST_CHECKED_STORAGE_KEY);
      }
    } catch {}
  }, [updateLastCheckedAt]);

  useEffect(() => {
    try {
      localStorage.setItem(UPDATE_AUTO_INSTALL_IDLE_STORAGE_KEY, updateAutoInstallWhenIdle ? "1" : "0");
    } catch {}
  }, [updateAutoInstallWhenIdle]);

  useEffect(() => {
    try {
      localStorage.setItem(UPDATE_TELEMETRY_STORAGE_KEY, JSON.stringify(updateTelemetry.slice(0, 80)));
    } catch {}
  }, [updateTelemetry]);

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
      appendUpdateTelemetry("check.failed", msg, source === "startup-auto" ? "startup-auto" : "manual");
      if (!silent) {
        setRuntimeMsg(`Update check failed: ${msg}`);
      }
    } finally {
      setIsCheckingUpdate(false);
    }
  }, [appendUpdateTelemetry, isCheckingUpdate, setRuntimeMsg, updateRemindUntilMs, updateRepoSlug, updateSkipVersion]);

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
      source === "idle-auto" ? "idle-auto" : "manual",
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
        source === "idle-auto" ? "idle-auto" : "manual",
      );
      setRuntimeMsg(
        autoExit
          ? `${result} Dictum will close to complete installation.`
          : `${result} Close Dictum to finish update if prompted.`,
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      appendUpdateTelemetry(
        "install.failed",
        msg,
        source === "idle-auto" ? "idle-auto" : "manual",
      );
      setRuntimeMsg(`Update install failed: ${msg}`);
    } finally {
      setIsInstallingUpdate(false);
    }
  }, [appendUpdateTelemetry, isInstallingUpdate, setRuntimeMsg, updateInfo]);

  const handleRemindLater = useCallback(() => {
    const remindUntil = Date.now() + (24 * 60 * 60 * 1000);
    setUpdateRemindUntilMs(remindUntil);
    appendUpdateTelemetry("defer.remind_later", `Snoozed until ${new Date(remindUntil).toISOString()}.`, "manual");
    setRuntimeMsg(`Update reminder snoozed until ${new Date(remindUntil).toLocaleString()}.`);
  }, [appendUpdateTelemetry, setRuntimeMsg]);

  const handleSkipUpdateVersion = useCallback(() => {
    if (!updateInfo?.latestVersion) return;
    setUpdateSkipVersion(updateInfo.latestVersion);
    appendUpdateTelemetry("defer.skip_version", `Skipped ${updateInfo.latestVersion}.`, "manual");
    setRuntimeMsg(`Skipped update ${updateInfo.latestVersion}.`);
  }, [appendUpdateTelemetry, setRuntimeMsg, updateInfo]);

  const clearUpdateDeferrals = useCallback(() => {
    setUpdateSkipVersion(null);
    setUpdateRemindUntilMs(0);
    appendUpdateTelemetry("defer.cleared", "Cleared skip/snooze preferences.", "manual");
    setRuntimeMsg("Cleared update skip/reminder preferences.");
  }, [appendUpdateTelemetry, setRuntimeMsg]);

  const handleExportUpdateTelemetry = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(JSON.stringify(updateTelemetry, null, 2));
      setUpdateLogCopied("done");
      setRuntimeMsg("Copied updater telemetry log to clipboard.");
    } catch {
      setUpdateLogCopied("error");
      setRuntimeMsg("Failed to copy updater telemetry log.");
    }
  }, [setRuntimeMsg, updateTelemetry]);

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

  return {
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
  };
}
