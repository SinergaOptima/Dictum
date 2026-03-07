"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type {
  DictionaryEntry,
  HistoryItem,
  PerfSnapshot,
  SnippetEntry,
  StatsPayload,
} from "@shared/ipc_types";
import {
  getDictionary,
  getHistory,
  getPerfSnapshot,
  getSnippets,
  getStats,
} from "@/lib/tauri";

type Tab = "live" | "history" | "stats" | "dictionary" | "snippets" | "settings";

type PanelLoadingState = {
  history: boolean;
  stats: boolean;
  dictionary: boolean;
  snippets: boolean;
};

const PANEL_CACHE_TTL_MS = 30_000;
const HISTORY_SEARCH_DEBOUNCE_MS = 180;

export function usePanelData(tab: Tab, historyQuery: string) {
  const [historyItems, setHistoryItems] = useState<HistoryItem[]>([]);
  const [stats, setStats] = useState<StatsPayload | null>(null);
  const [perfSnapshot, setPerfSnapshot] = useState<PerfSnapshot | null>(null);
  const [dictionary, setDictionary] = useState<DictionaryEntry[]>([]);
  const [snippets, setSnippets] = useState<SnippetEntry[]>([]);
  const [panelLoading, setPanelLoading] = useState<PanelLoadingState>({
    history: false,
    stats: false,
    dictionary: false,
    snippets: false,
  });
  const panelLoadedAtRef = useRef<Record<keyof PanelLoadingState, number>>({
    history: 0,
    stats: 0,
    dictionary: 0,
    snippets: 0,
  });

  const setPanelBusy = useCallback((panel: keyof PanelLoadingState, busy: boolean) => {
    setPanelLoading((prev) => (prev[panel] === busy ? prev : { ...prev, [panel]: busy }));
  }, []);

  const shouldUsePanelCache = useCallback(
    (panel: keyof PanelLoadingState, force: boolean) =>
      !force && Date.now() - panelLoadedAtRef.current[panel] < PANEL_CACHE_TTL_MS,
    [],
  );

  const refreshHistory = useCallback(async (force = false, query = historyQuery.trim()) => {
    if (!force && !query && shouldUsePanelCache("history", false)) {
      return;
    }
    setPanelBusy("history", true);
    try {
      const page = await getHistory(1, 100, query || null);
      setHistoryItems(page.items);
      panelLoadedAtRef.current.history = Date.now();
    } finally {
      setPanelBusy("history", false);
    }
  }, [historyQuery, setPanelBusy, shouldUsePanelCache]);

  const refreshStats = useCallback(async (force = false) => {
    if (!force && shouldUsePanelCache("stats", false)) {
      return;
    }
    setPanelBusy("stats", true);
    try {
      const [statsData, perfData] = await Promise.all([getStats(30), getPerfSnapshot()]);
      setStats(statsData);
      setPerfSnapshot(perfData);
      panelLoadedAtRef.current.stats = Date.now();
    } finally {
      setPanelBusy("stats", false);
    }
  }, [setPanelBusy, shouldUsePanelCache]);

  const refreshDictionary = useCallback(async (force = false) => {
    if (!force && shouldUsePanelCache("dictionary", false)) {
      return;
    }
    setPanelBusy("dictionary", true);
    try {
      setDictionary(await getDictionary());
      panelLoadedAtRef.current.dictionary = Date.now();
    } finally {
      setPanelBusy("dictionary", false);
    }
  }, [setPanelBusy, shouldUsePanelCache]);

  const refreshSnippets = useCallback(async (force = false) => {
    if (!force && shouldUsePanelCache("snippets", false)) {
      return;
    }
    setPanelBusy("snippets", true);
    try {
      setSnippets(await getSnippets());
      panelLoadedAtRef.current.snippets = Date.now();
    } finally {
      setPanelBusy("snippets", false);
    }
  }, [setPanelBusy, shouldUsePanelCache]);

  useEffect(() => {
    if (tab === "stats") void refreshStats();
    if (tab === "dictionary") void refreshDictionary();
    if (tab === "snippets") void refreshSnippets();
  }, [tab, refreshDictionary, refreshSnippets, refreshStats]);

  useEffect(() => {
    if (tab !== "history") return;
    const timer = window.setTimeout(() => {
      void refreshHistory(true, historyQuery.trim());
    }, HISTORY_SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [historyQuery, refreshHistory, tab]);

  return {
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
  };
}
