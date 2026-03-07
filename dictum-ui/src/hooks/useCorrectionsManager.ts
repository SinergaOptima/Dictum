"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import type { ActiveAppContext, AppProfile, LearnedCorrection } from "@shared/ipc_types";
import {
  deleteLearnedCorrection,
  getLearnedCorrections,
  learnCorrection,
  pruneLearnedCorrections,
} from "@/lib/tauri";

type UseCorrectionsManagerOptions = {
  setRuntimeMsg: (msg: string | null) => void;
  activeAppContext: ActiveAppContext | null;
  currentDictationMode: "conversation" | "coding" | "command";
  appProfiles: AppProfile[];
};

export type CorrectionScope = "global" | "mode" | "profile";
export type CorrectionFilterScope = "all" | "current" | CorrectionScope;
export type CorrectionSort = "best" | "recent" | "hits" | "heard";

type CorrectionContext = {
  modeAffinity: "conversation" | "coding" | "command" | null;
  appProfileAffinity: string | null;
  scope: CorrectionScope;
};

type EditingCorrection = {
  original: LearnedCorrection;
  heard: string;
  corrected: string;
  scope: CorrectionScope;
};

export type CorrectionHealthSummary = {
  totalRules: number;
  globalRules: number;
  modeRules: number;
  profileRules: number;
  currentContextRules: number;
  unusedRules: number;
  orphanedProfileRules: number;
  staleRules: number;
};

function normalizeImportedCorrection(
  rule: Partial<LearnedCorrection>,
  index: number,
): LearnedCorrection {
  const heard = typeof rule.heard === "string" ? rule.heard.trim() : "";
  const corrected = typeof rule.corrected === "string" ? rule.corrected.trim() : "";
  const modeAffinity = rule.modeAffinity == null ? null : String(rule.modeAffinity).trim().toLowerCase();
  const appProfileAffinity =
    typeof rule.appProfileAffinity === "string" && rule.appProfileAffinity.trim()
      ? rule.appProfileAffinity.trim()
      : null;
  if (!heard) {
    throw new Error(`Correction ${index + 1}: missing heard text.`);
  }
  if (!corrected) {
    throw new Error(`Correction ${index + 1}: missing corrected text.`);
  }
  if (
    modeAffinity !== null &&
    modeAffinity !== "conversation" &&
    modeAffinity !== "coding" &&
    modeAffinity !== "command"
  ) {
    throw new Error(`Correction ${index + 1}: invalid modeAffinity "${modeAffinity}".`);
  }
  return {
    heard,
    corrected,
    hits: Math.max(1, rule.hits ?? 1),
    modeAffinity,
    appProfileAffinity,
    lastUsedAt: typeof rule.lastUsedAt === "string" && rule.lastUsedAt.trim() ? rule.lastUsedAt : null,
  };
}

function resolveRuleScope(rule: LearnedCorrection): CorrectionScope {
  if (rule.appProfileAffinity) return "profile";
  if (rule.modeAffinity) return "mode";
  return "global";
}

function resolveCorrectionContext(
  scope: CorrectionScope,
  currentDictationMode: "conversation" | "coding" | "command",
  activeAppContext: ActiveAppContext | null,
): CorrectionContext {
  if (scope === "global") {
    return { modeAffinity: null, appProfileAffinity: null, scope };
  }
  if (scope === "mode") {
    return { modeAffinity: currentDictationMode, appProfileAffinity: null, scope };
  }
  return {
    modeAffinity: (activeAppContext?.dictationMode || currentDictationMode) as
      | "conversation"
      | "coding"
      | "command",
    appProfileAffinity: activeAppContext?.matchedProfileId || null,
    scope,
  };
}

function matchesCurrentContext(
  rule: LearnedCorrection,
  activeAppContext: ActiveAppContext | null,
  currentDictationMode: "conversation" | "coding" | "command",
) {
  const activeMode = activeAppContext?.dictationMode || currentDictationMode;
  const activeProfileId = activeAppContext?.matchedProfileId || null;
  if (rule.appProfileAffinity) {
    return rule.appProfileAffinity === activeProfileId;
  }
  if (rule.modeAffinity) {
    return rule.modeAffinity === activeMode;
  }
  return true;
}

export function useCorrectionsManager({
  setRuntimeMsg,
  activeAppContext,
  currentDictationMode,
  appProfiles,
}: UseCorrectionsManagerOptions) {
  const [learnedCorrections, setLearnedCorrections] = useState<LearnedCorrection[]>([]);
  const [correctionsCopied, setCorrectionsCopied] = useState<"idle" | "done" | "error">("idle");
  const [correctionsImportText, setCorrectionsImportText] = useState("");
  const [correctionFilter, setCorrectionFilter] = useState("");
  const [correctionHeardInput, setCorrectionHeardInput] = useState("");
  const [correctionFixedInput, setCorrectionFixedInput] = useState("");
  const [correctionScope, setCorrectionScope] = useState<CorrectionScope>("profile");
  const [correctionFilterScope, setCorrectionFilterScope] = useState<CorrectionFilterScope>("current");
  const [correctionSort, setCorrectionSort] = useState<CorrectionSort>("best");
  const [editingCorrection, setEditingCorrection] = useState<EditingCorrection | null>(null);

  useEffect(() => {
    getLearnedCorrections()
      .then((rules) => setLearnedCorrections(rules))
      .catch((err) => console.warn("Could not fetch learned corrections:", err));
  }, []);

  useEffect(() => {
    if (correctionsCopied === "idle") return;
    const timer = window.setTimeout(() => setCorrectionsCopied("idle"), 1600);
    return () => window.clearTimeout(timer);
  }, [correctionsCopied]);

  const activeCorrectionContext = useMemo(
    () => resolveCorrectionContext(correctionScope, currentDictationMode, activeAppContext),
    [activeAppContext, correctionScope, currentDictationMode],
  );

  const profileIds = useMemo(() => new Set(appProfiles.map((profile) => profile.id)), [appProfiles]);

  const correctionHealthSummary = useMemo<CorrectionHealthSummary>(() => {
    let globalRules = 0;
    let modeRules = 0;
    let profileRules = 0;
    let currentContextRules = 0;
    let unusedRules = 0;
    let orphanedProfileRules = 0;
    let staleRules = 0;
    const ninetyDaysMs = 90 * 24 * 60 * 60 * 1000;

    for (const rule of learnedCorrections) {
      const scope = resolveRuleScope(rule);
      if (scope === "global") globalRules += 1;
      if (scope === "mode") modeRules += 1;
      if (scope === "profile") profileRules += 1;
      if (matchesCurrentContext(rule, activeAppContext, currentDictationMode)) {
        currentContextRules += 1;
      }
      if (rule.hits <= 1 && !rule.lastUsedAt) {
        unusedRules += 1;
      }
      if (rule.appProfileAffinity && !profileIds.has(rule.appProfileAffinity)) {
        orphanedProfileRules += 1;
      }
      if (rule.hits <= 2 && rule.lastUsedAt) {
        const ageMs = Date.now() - new Date(rule.lastUsedAt).getTime();
        if (Number.isFinite(ageMs) && ageMs >= ninetyDaysMs) {
          staleRules += 1;
        }
      }
    }

    return {
      totalRules: learnedCorrections.length,
      globalRules,
      modeRules,
      profileRules,
      currentContextRules,
      unusedRules,
      orphanedProfileRules,
      staleRules,
    };
  }, [activeAppContext, appProfiles, currentDictationMode, learnedCorrections, profileIds]);

  const filteredLearnedCorrections = useMemo(() => {
    const query = correctionFilter.trim().toLowerCase();
    const scoped = learnedCorrections.filter((rule) => {
      if (correctionFilterScope === "all") return true;
      if (correctionFilterScope === "current") {
        return matchesCurrentContext(rule, activeAppContext, currentDictationMode);
      }
      return resolveRuleScope(rule) === correctionFilterScope;
    });
    const queried = query
      ? scoped.filter(
          (rule) =>
            rule.heard.toLowerCase().includes(query) ||
            rule.corrected.toLowerCase().includes(query) ||
            (rule.appProfileAffinity || "").toLowerCase().includes(query) ||
            (rule.modeAffinity || "").toLowerCase().includes(query),
        )
      : scoped;
    return [...queried].sort((a, b) => {
      switch (correctionSort) {
        case "recent":
          return (b.lastUsedAt || "").localeCompare(a.lastUsedAt || "") || b.hits - a.hits;
        case "hits":
          return b.hits - a.hits || (b.lastUsedAt || "").localeCompare(a.lastUsedAt || "");
        case "heard":
          return a.heard.localeCompare(b.heard) || a.corrected.localeCompare(b.corrected);
        case "best":
        default: {
          const aContext = matchesCurrentContext(a, activeAppContext, currentDictationMode) ? 1 : 0;
          const bContext = matchesCurrentContext(b, activeAppContext, currentDictationMode) ? 1 : 0;
          return (
            bContext - aContext ||
            b.hits - a.hits ||
            (b.lastUsedAt || "").localeCompare(a.lastUsedAt || "") ||
            a.heard.localeCompare(b.heard)
          );
        }
      }
    });
  }, [
    activeAppContext,
    correctionFilter,
    correctionFilterScope,
    correctionSort,
    currentDictationMode,
    learnedCorrections,
  ]);

  const upsertCorrection = useCallback(
    async (
      heard: string,
      corrected: string,
      modeAffinity: string | null,
      appProfileAffinity: string | null,
    ) => {
      return learnCorrection(heard, corrected, modeAffinity, appProfileAffinity);
    },
    [],
  );

  const handleLearnCorrection = useCallback(async () => {
    const heard = correctionHeardInput.trim();
    const corrected = correctionFixedInput.trim();
    if (!heard || !corrected) {
      setRuntimeMsg("Enter both heard and corrected text.");
      return;
    }
    if (
      !editingCorrection &&
      correctionScope === "profile" &&
      !activeAppContext?.matchedProfileId
    ) {
      setRuntimeMsg("No active app profile is matched right now. Switch to Mode or Global, or focus an app with a saved profile.");
      return;
    }
    try {
      let rules: LearnedCorrection[];
      if (editingCorrection) {
        await deleteLearnedCorrection(
          editingCorrection.original.heard,
          editingCorrection.original.corrected,
          editingCorrection.original.modeAffinity ?? null,
          editingCorrection.original.appProfileAffinity ?? null,
        );
      }
      rules = await upsertCorrection(
        heard,
        corrected,
        activeCorrectionContext.modeAffinity,
        activeCorrectionContext.appProfileAffinity,
      );
      setLearnedCorrections(rules);
      setCorrectionHeardInput("");
      setCorrectionFixedInput("");
      setEditingCorrection(null);
      setRuntimeMsg(
        editingCorrection
          ? `Updated correction: "${heard}" -> "${corrected}".`
          : `Learned correction: "${heard}" -> "${corrected}".`,
      );
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(
        `${editingCorrection ? "Failed to update" : "Failed to learn"} correction: ${msg}`,
      );
    }
  }, [
    activeCorrectionContext.appProfileAffinity,
    activeCorrectionContext.modeAffinity,
    correctionFixedInput,
    correctionHeardInput,
    correctionScope,
    editingCorrection,
    activeAppContext?.matchedProfileId,
    setRuntimeMsg,
    upsertCorrection,
  ]);

  const applyCorrection = useCallback(
    async (
      heard: string,
      corrected: string,
      modeAffinity?: string | null,
      appProfileAffinity?: string | null,
    ) => {
      try {
        const rules = await upsertCorrection(
          heard,
          corrected,
          modeAffinity ?? activeCorrectionContext.modeAffinity,
          appProfileAffinity ?? activeCorrectionContext.appProfileAffinity,
        );
        setLearnedCorrections(rules);
        return rules;
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        throw new Error(msg);
      }
    },
    [activeCorrectionContext.appProfileAffinity, activeCorrectionContext.modeAffinity, upsertCorrection],
  );

  const handleDeleteCorrection = useCallback(
    async (rule: LearnedCorrection) => {
      try {
        const rules = await deleteLearnedCorrection(
          rule.heard,
          rule.corrected,
          rule.modeAffinity ?? null,
          rule.appProfileAffinity ?? null,
        );
        setLearnedCorrections(rules);
        if (
          editingCorrection &&
          editingCorrection.original.heard === rule.heard &&
          editingCorrection.original.corrected === rule.corrected &&
          editingCorrection.original.modeAffinity === rule.modeAffinity &&
          editingCorrection.original.appProfileAffinity === rule.appProfileAffinity
        ) {
          setEditingCorrection(null);
          setCorrectionHeardInput("");
          setCorrectionFixedInput("");
        }
        setRuntimeMsg(`Removed correction for "${rule.heard}".`);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setRuntimeMsg(`Failed to remove correction: ${msg}`);
      }
    },
    [editingCorrection, setRuntimeMsg],
  );

  const handleStartEditingCorrection = useCallback((rule: LearnedCorrection) => {
    const scope = resolveRuleScope(rule);
    setEditingCorrection({
      original: rule,
      heard: rule.heard,
      corrected: rule.corrected,
      scope,
    });
    setCorrectionHeardInput(rule.heard);
    setCorrectionFixedInput(rule.corrected);
    setCorrectionScope(scope);
  }, []);

  const handleCancelEditingCorrection = useCallback(() => {
    setEditingCorrection(null);
    setCorrectionHeardInput("");
    setCorrectionFixedInput("");
    setCorrectionScope("profile");
  }, []);

  const handleCopyCorrections = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(JSON.stringify(learnedCorrections, null, 2));
      setCorrectionsCopied("done");
      setRuntimeMsg("Copied learned corrections JSON to clipboard.");
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setCorrectionsCopied("error");
      setRuntimeMsg(`Failed to copy learned corrections: ${msg}`);
    }
  }, [learnedCorrections, setRuntimeMsg]);

  const handleImportCorrections = useCallback(async () => {
    const raw = correctionsImportText.trim();
    if (!raw) {
      setRuntimeMsg("Paste learned corrections JSON first.");
      return;
    }
    try {
      const parsed = JSON.parse(raw) as Partial<LearnedCorrection>[];
      if (!Array.isArray(parsed)) {
        throw new Error("Expected a JSON array of learned corrections.");
      }
      if (parsed.length === 0) {
        throw new Error("No learned corrections found in the pasted JSON.");
      }
      const normalized = parsed.map((rule, index) => normalizeImportedCorrection(rule, index));
      normalized.forEach((rule, index) => {
        if (rule.appProfileAffinity && !profileIds.has(rule.appProfileAffinity)) {
          throw new Error(
            `Correction ${index + 1}: appProfileAffinity "${rule.appProfileAffinity}" does not match any saved app profile.`,
          );
        }
      });
      let latest = learnedCorrections;
      for (const rule of normalized) {
        latest = await upsertCorrection(
          rule.heard,
          rule.corrected,
          rule.modeAffinity ?? null,
          rule.appProfileAffinity ?? null,
        );
      }
      setLearnedCorrections(latest);
      setCorrectionsImportText("");
      setRuntimeMsg(`Imported ${normalized.length} learned correction${normalized.length === 1 ? "" : "s"}.`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRuntimeMsg(`Failed to import learned corrections: ${msg}`);
    }
  }, [correctionsImportText, learnedCorrections, profileIds, setRuntimeMsg, upsertCorrection]);

  const handlePruneCorrections = useCallback(
    async (mode: "unused" | "orphaned" | "stale") => {
      try {
        const result = await pruneLearnedCorrections(
          mode === "unused",
          mode === "orphaned",
          mode === "stale",
        );
        setLearnedCorrections(result.rules);
        const removed =
          result.removedUnused + result.removedOrphanedProfiles + result.removedStale;
        setRuntimeMsg(
          removed > 0
            ? `Pruned ${removed} correction rule${removed === 1 ? "" : "s"} (${result.removedUnused} unused, ${result.removedOrphanedProfiles} orphaned, ${result.removedStale} stale).`
            : "No correction rules matched that prune filter.",
        );
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setRuntimeMsg(`Failed to prune corrections: ${msg}`);
      }
    },
    [setRuntimeMsg],
  );

  return {
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
    editingCorrection: editingCorrection?.original ?? null,
    applyCorrection,
    handleLearnCorrection,
    handleDeleteCorrection,
    handleStartEditingCorrection,
    handleCancelEditingCorrection,
    handleCopyCorrections,
    handleImportCorrections,
    handlePruneCorrections,
  };
}
