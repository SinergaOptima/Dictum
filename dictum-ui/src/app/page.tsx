"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useActivity } from "@/hooks/useActivity";
import { useAudioDevices } from "@/hooks/useAudioDevices";
import { useEngine } from "@/hooks/useEngine";
import { useTranscript } from "@/hooks/useTranscript";
import type {
  DictionaryEntry,
  HistoryItem,
  PrivacySettings,
  SnippetEntry,
  StatsPayload,
} from "@shared/ipc_types";
import {
  deleteDictionary,
  deleteHistory,
  deleteSnippet,
  getDictionary,
  getHistory,
  getPreferredInputDevice,
  getPrivacySettings,
  getRuntimeSettings,
  getSnippets,
  getStats,
  setPreferredInputDevice,
  setRuntimeSettings,
  upsertDictionary,
  upsertSnippet,
} from "@/lib/tauri";

type Tab = "live" | "history" | "stats" | "dictionary" | "snippets" | "settings";

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return (
    target.isContentEditable ||
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT"
  );
}

const MODEL_PROFILE_OPTIONS = [
  { value: "large-v3-turbo", label: "Large v3 Turbo (default)" },
  { value: "small", label: "Small" },
  { value: "small.en", label: "Small English" },
  { value: "base.en", label: "Base English" },
  { value: "tiny.en", label: "Tiny English" },
];

const ORT_EP_OPTIONS = [
  { value: "auto", label: "Auto" },
  { value: "directml", label: "DirectML (GPU)" },
  { value: "cpu", label: "CPU" },
];

const LANGUAGE_HINT_OPTIONS = [
  { value: "auto", label: "Auto" },
  { value: "english", label: "English" },
  { value: "mandarin", label: "Mandarin" },
  { value: "russian", label: "Russian" },
];

export default function Home() {
  const { isListening, status, startEngine, stopEngine, error } = useEngine();
  const { isSpeech, level } = useActivity();
  const { segments, clearSegments } = useTranscript();
  const { defaultDevice, devices, loading } = useAudioDevices();

  const feedRef = useRef<HTMLDivElement>(null);
  const pushToTalkRef = useRef(false);

  const [tab, setTab] = useState<Tab>("live");
  const [copyState, setCopyState] = useState<"idle" | "done" | "error">("idle");
  const [selectedDeviceName, setSelectedDeviceName] = useState<string | null>(null);
  const [modelProfile, setModelProfile] = useState("large-v3-turbo");
  const [ortEp, setOrtEp] = useState("auto");
  const [languageHint, setLanguageHint] = useState("auto");
  const [cloudOptIn, setCloudOptIn] = useState(false);
  const [historyEnabled, setHistoryEnabled] = useState(true);
  const [retentionDays, setRetentionDays] = useState(90);
  const [runtimeMsg, setRuntimeMsg] = useState<string | null>(null);

  const [historyItems, setHistoryItems] = useState<HistoryItem[]>([]);
  const [historyQuery, setHistoryQuery] = useState("");
  const [stats, setStats] = useState<StatsPayload | null>(null);
  const [dictionary, setDictionary] = useState<DictionaryEntry[]>([]);
  const [snippets, setSnippets] = useState<SnippetEntry[]>([]);
  const [dictTerm, setDictTerm] = useState("");
  const [dictAliases, setDictAliases] = useState("");
  const [dictLanguage, setDictLanguage] = useState("");
  const [snippetTrigger, setSnippetTrigger] = useState("");
  const [snippetExpansion, setSnippetExpansion] = useState("");
  const [snippetMode, setSnippetMode] = useState<"slash" | "phrase">("slash");

  const copyText = useMemo(() => {
    const finals = segments.filter((seg) => seg.kind === "final");
    const source = finals.length > 0 ? finals : segments;
    return source.map((seg) => seg.text.trim()).filter(Boolean).join("\n");
  }, [segments]);

  useEffect(() => {
    if (feedRef.current) {
      feedRef.current.scrollTop = feedRef.current.scrollHeight;
    }
  }, [segments]);

  useEffect(() => {
    getPreferredInputDevice()
      .then((name) => setSelectedDeviceName(name))
      .catch((err) => console.warn("Could not fetch preferred input device:", err));
  }, []);

  useEffect(() => {
    Promise.all([getRuntimeSettings(), getPrivacySettings()])
      .then(([runtime, privacy]) => {
        setModelProfile(runtime.modelProfile || "large-v3-turbo");
        setOrtEp(runtime.ortEp || "auto");
        setLanguageHint(runtime.languageHint || "auto");
        setCloudOptIn(privacy.cloudOptIn);
        setHistoryEnabled(privacy.historyEnabled);
        setRetentionDays(privacy.retentionDays ?? 90);
      })
      .catch((err) => console.warn("Could not fetch runtime/privacy settings:", err));
  }, []);

  useEffect(() => {
    if (!selectedDeviceName && defaultDevice?.name) {
      setSelectedDeviceName(defaultDevice.name);
    }
  }, [defaultDevice, selectedDeviceName]);

  useEffect(() => {
    if (copyState === "idle") return;
    const timer = window.setTimeout(() => setCopyState("idle"), 1400);
    return () => window.clearTimeout(timer);
  }, [copyState]);

  const copyTranscript = useCallback(async () => {
    if (!copyText) return;
    try {
      await navigator.clipboard.writeText(copyText);
      setCopyState("done");
    } catch {
      setCopyState("error");
    }
  }, [copyText]);

  const handleToggle = useCallback(async () => {
    if (isListening) {
      await stopEngine();
    } else {
      await startEngine(selectedDeviceName);
    }
  }, [isListening, startEngine, stopEngine, selectedDeviceName]);

  const applyRuntime = useCallback(
    async (overrides?: Partial<{
      modelProfile: string;
      ortEp: string;
      languageHint: string;
      cloudOptIn: boolean;
      historyEnabled: boolean;
      retentionDays: number;
    }>) => {
      const next = {
        modelProfile,
        ortEp,
        languageHint,
        cloudOptIn,
        historyEnabled,
        retentionDays,
        ...overrides,
      };
      try {
        const updated = await setRuntimeSettings(
          next.modelProfile,
          next.ortEp,
          next.languageHint,
          next.cloudOptIn,
          next.historyEnabled,
          next.retentionDays,
        );
        setModelProfile(updated.modelProfile || "large-v3-turbo");
        setOrtEp(updated.ortEp || "auto");
        setLanguageHint(updated.languageHint || "auto");
        setCloudOptIn(updated.cloudOptIn);
        setHistoryEnabled(updated.historyEnabled);
        setRetentionDays(updated.retentionDays);
        setRuntimeMsg("Saved.");
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setRuntimeMsg(`Failed to save settings: ${msg}`);
      }
    },
    [modelProfile, ortEp, languageHint, cloudOptIn, historyEnabled, retentionDays],
  );

  const refreshHistory = useCallback(async () => {
    const page = await getHistory(1, 100, historyQuery || null);
    setHistoryItems(page.items);
  }, [historyQuery]);

  const refreshStats = useCallback(async () => {
    const data = await getStats(30);
    setStats(data);
  }, []);

  const refreshDictionary = useCallback(async () => {
    setDictionary(await getDictionary());
  }, []);

  const refreshSnippets = useCallback(async () => {
    setSnippets(await getSnippets());
  }, []);

  useEffect(() => {
    if (tab === "history") void refreshHistory();
    if (tab === "stats") void refreshStats();
    if (tab === "dictionary") void refreshDictionary();
    if (tab === "snippets") void refreshSnippets();
  }, [tab, refreshHistory, refreshStats, refreshDictionary, refreshSnippets]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const isMod = event.ctrlKey || event.metaKey;

      if (isMod && event.key.toLowerCase() === "enter") {
        if (!isEditableTarget(event.target)) {
          event.preventDefault();
          void handleToggle();
        }
        return;
      }

      if (isMod && event.shiftKey && event.key.toLowerCase() === "c") {
        if (!isEditableTarget(event.target)) {
          event.preventDefault();
          void copyTranscript();
        }
        return;
      }

      if (isMod && event.key.toLowerCase() === "l") {
        if (!isEditableTarget(event.target)) {
          event.preventDefault();
          clearSegments();
        }
        return;
      }

      if (event.code !== "Space" || event.repeat || isEditableTarget(event.target)) return;
      event.preventDefault();
      if (isMod || event.altKey) return;
      if (pushToTalkRef.current || isListening) return;

      pushToTalkRef.current = true;
      void startEngine(selectedDeviceName).catch(() => {
        pushToTalkRef.current = false;
      });
    };

    const onKeyUp = (event: KeyboardEvent) => {
      if (event.code !== "Space" || !pushToTalkRef.current) return;
      event.preventDefault();
      pushToTalkRef.current = false;
      if (isListening) void stopEngine();
    };

    const onBlur = () => {
      if (!pushToTalkRef.current) return;
      pushToTalkRef.current = false;
      if (isListening) void stopEngine();
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
    };
  }, [clearSegments, copyTranscript, handleToggle, isListening, selectedDeviceName, startEngine, stopEngine]);

  const statusLabel =
    status === "listening" && isSpeech ? "Hearing" :
    status === "listening" ? "Listening" :
    status === "warmingup" ? "Loading" :
    status === "idle" ? "Ready" :
    status === "stopped" ? "Stopped" :
    status === "error" ? "Error" :
    status;

  const hasSegments = segments.length > 0;

  return (
    <div className="app-layout">
      <div className="theme-bg" aria-hidden />

      <header className="app-header">
        <span className="app-brand">Dictum</span>
        <span
          className={`app-status-badge${isListening ? " is-listening" : ""}${status === "error" ? " is-error" : ""}`}
          role="status"
          aria-live="polite"
        >
          {statusLabel}
        </span>
        <div className="tabs-row">
          {(["live", "history", "stats", "dictionary", "snippets", "settings"] as Tab[]).map((value) => (
            <button
              key={value}
              className={`tab-btn${tab === value ? " active" : ""}`}
              onClick={() => setTab(value)}
              data-no-drag
            >
              {value}
            </button>
          ))}
        </div>
        <div className="app-spacer" />
        {error && (
          <span className="error-banner" role="alert" title={error}>
            {error}
          </span>
        )}
      </header>

      {tab === "live" && (
        <>
          <div className="transcript-scroll selectable" ref={feedRef}>
            {hasSegments ? (
              <div className="transcript-feed">
                {segments.map((seg) => (
                  <p key={seg.id} className={seg.kind === "partial" ? "seg-partial" : "seg-final"}>
                    {seg.text}
                  </p>
                ))}
              </div>
            ) : (
              <div className="empty-state">
                <div className="empty-glyph" aria-hidden>D</div>
                <p className="empty-label">
                  {isListening ? "Listening for speech..." : "Your transcript will appear here"}
                </p>
                <p className="empty-hint">Hold Space · Ctrl+Enter · Ctrl+Shift+Space</p>
              </div>
            )}
          </div>

          <footer className="app-footer">
            <div className="device-select-wrap">
              <svg className="mic-icon" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden>
                <rect x="5.5" y="1" width="5" height="8" rx="2.5" stroke="currentColor" strokeWidth="1.4" />
                <path d="M3 8a5 5 0 0010 0" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
                <line x1="8" y1="13" x2="8" y2="15" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
              </svg>
              <select
                className="device-select"
                value={selectedDeviceName ?? ""}
                onChange={(e) => {
                  const next = e.target.value || null;
                  setSelectedDeviceName(next);
                  void setPreferredInputDevice(next).catch(console.error);
                }}
                disabled={isListening || loading}
                aria-label="Microphone input device"
              >
                {selectedDeviceName && !devices.some((d) => d.name === selectedDeviceName) && (
                  <option value={selectedDeviceName}>{selectedDeviceName} (unavailable)</option>
                )}
                {devices.map((d) => (
                  <option key={d.name} value={d.name}>
                    {d.name}{d.isDefault ? " *" : ""}
                  </option>
                ))}
              </select>
            </div>

            <div className="footer-spacer" />

            {hasSegments && (
              <>
                <button type="button" className="action-btn" onClick={() => void copyTranscript()} disabled={!copyText}>
                  {copyState === "done" ? "Copied" : copyState === "error" ? "Failed" : "Copy"}
                </button>
                <button type="button" className="action-btn" onClick={clearSegments}>
                  Clear
                </button>
              </>
            )}

            {isListening && (
              <div className="level-bars" aria-hidden>
                {Array.from({ length: 7 }).map((_, i) => {
                  const active = i < Math.ceil(level * 7);
                  return (
                    <span
                      key={i}
                      className={`level-bar${active ? " active" : ""}`}
                      style={active ? { height: `${4 + level * 13 + i}px` } : undefined}
                    />
                  );
                })}
              </div>
            )}

            <button
              type="button"
              className={`record-btn${isListening ? " record-btn--live" : ""}`}
              onClick={() => void handleToggle()}
              aria-pressed={isListening}
            >
              {isListening ? <span className="record-stop" aria-hidden /> : <span className="record-dot" aria-hidden />}
            </button>
          </footer>
        </>
      )}

      {tab !== "live" && (
        <div className="panel-scroll selectable" data-no-drag>
          {tab === "history" && (
            <section className="panel">
              <div className="panel-toolbar">
                <input
                  className="runtime-select panel-input"
                  placeholder="Search dictation history..."
                  value={historyQuery}
                  onChange={(e) => setHistoryQuery(e.target.value)}
                />
                <button className="action-btn" onClick={() => void refreshHistory()}>Refresh</button>
                <button
                  className="action-btn"
                  onClick={async () => {
                    await deleteHistory(null, retentionDays);
                    await refreshHistory();
                  }}
                >
                  Prune
                </button>
              </div>
              <div className="panel-list">
                {historyItems.map((item) => (
                  <article key={item.id} className="panel-card">
                    <div className="panel-meta">
                      <span>{new Date(item.createdAt).toLocaleString()}</span>
                      <span>{item.source}</span>
                      <span>{item.wordCount} words</span>
                    </div>
                    <p>{item.text}</p>
                    <button
                      className="action-btn"
                      onClick={async () => {
                        await deleteHistory([item.id], null);
                        await refreshHistory();
                      }}
                    >
                      Delete
                    </button>
                  </article>
                ))}
                {historyItems.length === 0 && <p className="empty-label">No history yet.</p>}
              </div>
            </section>
          )}

          {tab === "stats" && (
            <section className="panel">
              <div className="panel-toolbar">
                <button className="action-btn" onClick={() => void refreshStats()}>Refresh</button>
              </div>
              {stats ? (
                <div className="panel-grid">
                  <div className="stat-card"><b>{stats.totalUtterances}</b><span>Utterances</span></div>
                  <div className="stat-card"><b>{stats.totalWords}</b><span>Words</span></div>
                  <div className="stat-card"><b>{Math.round(stats.avgLatencyMs)} ms</b><span>Avg Latency</span></div>
                </div>
              ) : <p className="empty-label">No stats yet.</p>}
            </section>
          )}

          {tab === "dictionary" && (
            <section className="panel">
              <div className="panel-toolbar">
                <input className="runtime-select panel-input" placeholder="Canonical term" value={dictTerm} onChange={(e) => setDictTerm(e.target.value)} />
                <input className="runtime-select panel-input" placeholder="Aliases (comma-separated)" value={dictAliases} onChange={(e) => setDictAliases(e.target.value)} />
                <input className="runtime-select panel-input" placeholder="Language (optional)" value={dictLanguage} onChange={(e) => setDictLanguage(e.target.value)} />
                <button
                  className="action-btn"
                  onClick={async () => {
                    if (!dictTerm.trim()) return;
                    await upsertDictionary({
                      id: "",
                      term: dictTerm.trim(),
                      aliases: dictAliases.split(",").map((v) => v.trim()).filter(Boolean),
                      language: dictLanguage.trim() || null,
                      enabled: true,
                      createdAt: "",
                      updatedAt: "",
                    });
                    setDictTerm("");
                    setDictAliases("");
                    setDictLanguage("");
                    await refreshDictionary();
                  }}
                >
                  Add
                </button>
              </div>
              <div className="panel-list">
                {dictionary.map((entry) => (
                  <article key={entry.id} className="panel-card">
                    <div className="panel-meta">
                      <span>{entry.term}</span>
                      <span>{entry.language ?? "any"}</span>
                    </div>
                    <p>{entry.aliases.join(", ") || "No aliases"}</p>
                    <button className="action-btn" onClick={async () => { await deleteDictionary(entry.id); await refreshDictionary(); }}>Delete</button>
                  </article>
                ))}
                {dictionary.length === 0 && <p className="empty-label">No dictionary entries yet.</p>}
              </div>
            </section>
          )}

          {tab === "snippets" && (
            <section className="panel">
              <div className="panel-toolbar">
                <input className="runtime-select panel-input" placeholder="Trigger (e.g. /email)" value={snippetTrigger} onChange={(e) => setSnippetTrigger(e.target.value)} />
                <input className="runtime-select panel-input" placeholder="Expansion text" value={snippetExpansion} onChange={(e) => setSnippetExpansion(e.target.value)} />
                <select className="runtime-select" value={snippetMode} onChange={(e) => setSnippetMode(e.target.value as "slash" | "phrase")}>
                  <option value="slash">slash</option>
                  <option value="phrase">phrase</option>
                </select>
                <button
                  className="action-btn"
                  onClick={async () => {
                    if (!snippetTrigger.trim() || !snippetExpansion.trim()) return;
                    await upsertSnippet({
                      id: "",
                      trigger: snippetTrigger.trim(),
                      expansion: snippetExpansion.trim(),
                      mode: snippetMode,
                      enabled: true,
                      createdAt: "",
                      updatedAt: "",
                    });
                    setSnippetTrigger("");
                    setSnippetExpansion("");
                    await refreshSnippets();
                  }}
                >
                  Add
                </button>
              </div>
              <div className="panel-list">
                {snippets.map((entry) => (
                  <article key={entry.id} className="panel-card">
                    <div className="panel-meta">
                      <span>{entry.trigger}</span>
                      <span>{entry.mode}</span>
                    </div>
                    <p>{entry.expansion}</p>
                    <button className="action-btn" onClick={async () => { await deleteSnippet(entry.id); await refreshSnippets(); }}>Delete</button>
                  </article>
                ))}
                {snippets.length === 0 && <p className="empty-label">No snippets yet.</p>}
              </div>
            </section>
          )}

          {tab === "settings" && (
            <section className="panel">
              <div className="panel-grid">
                <label className="runtime-field">
                  <span className="runtime-label">Model</span>
                  <select className="runtime-select" value={modelProfile} onChange={(e) => setModelProfile(e.target.value)}>
                    {MODEL_PROFILE_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
                  </select>
                </label>
                <label className="runtime-field">
                  <span className="runtime-label">Runtime</span>
                  <select className="runtime-select" value={ortEp} onChange={(e) => setOrtEp(e.target.value)}>
                    {ORT_EP_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
                  </select>
                </label>
                <label className="runtime-field">
                  <span className="runtime-label">Language</span>
                  <select className="runtime-select" value={languageHint} onChange={(e) => setLanguageHint(e.target.value)}>
                    {LANGUAGE_HINT_OPTIONS.map((opt) => <option key={opt.value} value={opt.value}>{opt.label}</option>)}
                  </select>
                </label>
                <label className="runtime-field">
                  <span className="runtime-label">Retention (days)</span>
                  <input className="runtime-select panel-input" type="number" min={1} max={3650} value={retentionDays} onChange={(e) => setRetentionDays(Number(e.target.value || 90))} />
                </label>
              </div>
              <div className="panel-toolbar">
                <label className="toggle"><input type="checkbox" checked={cloudOptIn} onChange={(e) => setCloudOptIn(e.target.checked)} />Cloud fallback (OpenAI)</label>
                <label className="toggle"><input type="checkbox" checked={historyEnabled} onChange={(e) => setHistoryEnabled(e.target.checked)} />Save history</label>
                <button className="action-btn" onClick={() => void applyRuntime()}>Save settings</button>
              </div>
              <p className="runtime-hint">
                {runtimeMsg ?? "Cloud fallback requires DICTUM_OPENAI_API_KEY in your environment."}
              </p>
            </section>
          )}
        </div>
      )}
    </div>
  );
}
