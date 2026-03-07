"use client";

import { useCallback, useEffect, useState } from "react";
import type { ActiveAppContext, AppProfile } from "@shared/ipc_types";
import { deleteAppProfile, getActiveAppContext, getAppProfiles, upsertAppProfile } from "@/lib/tauri";

type DictationMode = AppProfile["dictationMode"];

type AppProfilePreset = {
  name: string;
  appMatch: string;
  dictationMode: DictationMode;
  phraseBiasTerms: string[];
  postUtteranceRefine: boolean;
};

type UseAppProfilesOptions = {
  tab: string;
  setRuntimeMsg: (msg: string | null) => void;
};

function normalizeExecutableMatch(value: string, index?: number): string {
  const trimmed = value.trim().replace(/^["']+|["']+$/g, "");
  const appMatch = trimmed.split(/[\\/]/).pop()?.trim().toLowerCase() ?? "";
  const label = index == null ? "Profile" : `Profile ${index + 1}`;
  if (!appMatch) {
    throw new Error(`${label}: missing appMatch executable.`);
  }
  if (!appMatch.endsWith(".exe")) {
    throw new Error(
      `${label}: appMatch must resolve to a Windows executable like "cursor.exe".`,
    );
  }
  return appMatch;
}

function normalizeImportedProfile(profile: Partial<AppProfile>, index: number): AppProfile {
  const name = typeof profile.name === "string" ? profile.name.trim() : "";
  const appMatch =
    typeof profile.appMatch === "string" ? normalizeExecutableMatch(profile.appMatch, index) : "";
  const dictationMode = profile.dictationMode;
  if (!name) {
    throw new Error(`Profile ${index + 1}: missing name.`);
  }
  if (
    dictationMode !== "conversation" &&
    dictationMode !== "coding" &&
    dictationMode !== "command"
  ) {
    throw new Error(`Profile ${index + 1}: invalid dictationMode "${String(dictationMode)}".`);
  }
  const phraseBiasTerms = Array.isArray(profile.phraseBiasTerms)
    ? profile.phraseBiasTerms
        .filter((term): term is string => typeof term === "string")
        .map((term) => term.trim())
        .filter(Boolean)
    : [];

  return {
    id: typeof profile.id === "string" && profile.id.trim() ? profile.id : crypto.randomUUID(),
    name,
    appMatch,
    dictationMode,
    phraseBiasTerms,
    postUtteranceRefine: !!profile.postUtteranceRefine,
    enabled: profile.enabled ?? true,
  };
}

export function useAppProfiles({ tab, setRuntimeMsg }: UseAppProfilesOptions) {
  const [appProfiles, setAppProfiles] = useState<AppProfile[]>([]);
  const [activeAppContext, setActiveAppContext] = useState<ActiveAppContext | null>(null);
  const [editingAppProfileId, setEditingAppProfileId] = useState<string | null>(null);
  const [appProfileName, setAppProfileName] = useState("");
  const [appProfileMatch, setAppProfileMatch] = useState("");
  const [appProfileMode, setAppProfileMode] = useState<DictationMode>("coding");
  const [appProfileBiasTerms, setAppProfileBiasTerms] = useState("");
  const [appProfileRefine, setAppProfileRefine] = useState(false);
  const [appProfilesCopied, setAppProfilesCopied] = useState<"idle" | "done" | "error">("idle");
  const [appProfilesImportText, setAppProfilesImportText] = useState("");

  useEffect(() => {
    getAppProfiles()
      .then((profiles) => setAppProfiles(profiles))
      .catch((err) => console.warn("Could not fetch app profiles:", err));
  }, []);

  useEffect(() => {
    if (tab !== "settings") return;
    let cancelled = false;
    const refresh = () => {
      getActiveAppContext()
        .then((context) => {
          if (!cancelled) setActiveAppContext(context);
        })
        .catch((err) => console.warn("Could not fetch active app context:", err));
    };
    refresh();
    const timer = window.setInterval(refresh, 1200);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [tab]);

  useEffect(() => {
    if (appProfilesCopied === "idle") return;
    const timer = window.setTimeout(() => setAppProfilesCopied("idle"), 1600);
    return () => window.clearTimeout(timer);
  }, [appProfilesCopied]);

  const resetAppProfileEditor = useCallback(() => {
    setEditingAppProfileId(null);
    setAppProfileName("");
    setAppProfileMatch("");
    setAppProfileMode("coding");
    setAppProfileBiasTerms("");
    setAppProfileRefine(false);
  }, []);

  const handleSaveAppProfile = useCallback(async () => {
    const name = appProfileName.trim();
    if (!name || !appProfileMatch.trim()) {
      setRuntimeMsg("Enter both a profile name and an app executable match.");
      return;
    }
    try {
      const appMatch = normalizeExecutableMatch(appProfileMatch);
      const duplicate = appProfiles.find(
        (profile) =>
          profile.id !== editingAppProfileId &&
          profile.appMatch.trim().toLowerCase() === appMatch,
      );
      if (duplicate) {
        throw new Error(`Another app profile already uses ${appMatch}. Edit that profile instead.`);
      }
      const profiles = await upsertAppProfile({
        id: editingAppProfileId ?? crypto.randomUUID(),
        name,
        appMatch,
        dictationMode: appProfileMode,
        phraseBiasTerms: appProfileBiasTerms
          .split(/\r?\n|,/)
          .map((term) => term.trim())
          .filter(Boolean),
        postUtteranceRefine: appProfileRefine,
        enabled: true,
      });
      setAppProfiles(profiles);
      resetAppProfileEditor();
      setRuntimeMsg(`${editingAppProfileId ? "Updated" : "Saved"} app profile for ${appMatch}.`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to save app profile: ${msg}`);
    }
  }, [
    appProfileBiasTerms,
    appProfileMatch,
    appProfileMode,
    appProfileName,
    appProfileRefine,
    editingAppProfileId,
    resetAppProfileEditor,
    setRuntimeMsg,
  ]);

  const handleEditAppProfile = useCallback((profile: AppProfile) => {
    setEditingAppProfileId(profile.id);
    setAppProfileName(profile.name);
    setAppProfileMatch(profile.appMatch);
    setAppProfileMode(profile.dictationMode);
    setAppProfileBiasTerms(profile.phraseBiasTerms.join("\n"));
    setAppProfileRefine(profile.postUtteranceRefine);
    setRuntimeMsg(`Editing app profile "${profile.name}".`);
  }, [setRuntimeMsg]);

  const handleApplyAppProfilePreset = useCallback((preset: AppProfilePreset) => {
    setEditingAppProfileId(null);
    setAppProfileName(preset.name);
    setAppProfileMatch(preset.appMatch);
    setAppProfileMode(preset.dictationMode);
    setAppProfileBiasTerms(preset.phraseBiasTerms.join("\n"));
    setAppProfileRefine(preset.postUtteranceRefine);
    setRuntimeMsg(`Loaded ${preset.name} preset into the profile editor.`);
  }, [setRuntimeMsg]);

  const handleDeleteAppProfile = useCallback(async (id: string, name: string) => {
    try {
      const profiles = await deleteAppProfile(id);
      setAppProfiles(profiles);
      if (editingAppProfileId === id) {
        resetAppProfileEditor();
      }
      setRuntimeMsg(`Removed app profile "${name}".`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to delete app profile: ${msg}`);
    }
  }, [editingAppProfileId, resetAppProfileEditor, setRuntimeMsg]);

  const handleCopyAppProfiles = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(JSON.stringify(appProfiles, null, 2));
      setAppProfilesCopied("done");
      setRuntimeMsg("Copied app profiles JSON to clipboard.");
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setAppProfilesCopied("error");
      setRuntimeMsg(`Failed to copy app profiles: ${msg}`);
    }
  }, [appProfiles, setRuntimeMsg]);

  const handleImportAppProfiles = useCallback(async () => {
    const raw = appProfilesImportText.trim();
    if (!raw) {
      setRuntimeMsg("Paste exported app profiles JSON first.");
      return;
    }
    try {
      const parsed = JSON.parse(raw) as Partial<AppProfile>[];
      if (!Array.isArray(parsed)) {
        throw new Error("Expected a JSON array of app profiles.");
      }
      if (parsed.length === 0) {
        throw new Error("No app profiles found in the pasted JSON.");
      }
      const normalized = parsed.map((profile, index) => normalizeImportedProfile(profile, index));
      const seenIds = new Set<string>();
      const seenNames = new Set<string>();
      const seenMatches = new Set<string>();
      for (const profile of normalized) {
        if (seenIds.has(profile.id)) {
          throw new Error(`Duplicate profile id "${profile.id}" found in imported profiles.`);
        }
        seenIds.add(profile.id);
        const normalizedName = profile.name.trim().toLowerCase();
        if (seenNames.has(normalizedName)) {
          throw new Error(`Duplicate profile name "${profile.name}" found in imported profiles.`);
        }
        seenNames.add(normalizedName);
        if (seenMatches.has(profile.appMatch)) {
          throw new Error(`Duplicate appMatch "${profile.appMatch}" found in imported profiles.`);
        }
        seenMatches.add(profile.appMatch);
      }
      let latest = appProfiles;
      for (const profile of normalized) {
        latest = await upsertAppProfile(profile);
      }
      setAppProfiles(latest);
      setAppProfilesImportText("");
      resetAppProfileEditor();
      setRuntimeMsg(`Imported ${normalized.length} app profile${normalized.length === 1 ? "" : "s"}.`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to import app profiles: ${msg}`);
    }
  }, [appProfiles, appProfilesImportText, resetAppProfileEditor, setRuntimeMsg]);

  return {
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
  };
}
