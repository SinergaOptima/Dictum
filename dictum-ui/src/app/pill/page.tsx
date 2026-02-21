"use client";

import { useEffect, useState, type MouseEvent, type PointerEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useActivity } from "@/hooks/useActivity";
import { useEngine } from "@/hooks/useEngine";
import { getRuntimeSettings, listenTranscript } from "@/lib/tauri";

function boostPillLevel(level: number, sensitivity: number): number {
  const clamped = Math.min(20, Math.max(1, sensitivity));
  // Roughly 10x at very low input levels, then compress to preserve headroom.
  return Math.min(1, (level * clamped) / (1 + level * (clamped - 1)));
}

export default function PillPage() {
  const { isListening, status } = useEngine();
  const [visualizerSensitivity, setVisualizerSensitivity] = useState(10);
  const [activitySensitivity, setActivitySensitivity] = useState(4.2);
  const [activityNoiseGate, setActivityNoiseGate] = useState(0.0015);
  const [activityClipThreshold, setActivityClipThreshold] = useState(0.32);
  const [finalConfidence, setFinalConfidence] = useState<number | null>(null);
  const { isSpeech, level, isNoisy, isClipping } = useActivity({
    sensitivity: activitySensitivity,
    noiseGate: activityNoiseGate,
    clipThreshold: activityClipThreshold,
  });
  const isActive = isListening;

  useEffect(() => {
    document.body.classList.add("pill-mode");
    document.documentElement.classList.add("pill-mode");
    document.documentElement.style.background = "transparent";
    document.documentElement.style.backgroundColor = "transparent";
    return () => {
      document.body.classList.remove("pill-mode");
      document.documentElement.classList.remove("pill-mode");
      document.documentElement.style.background = "";
      document.documentElement.style.backgroundColor = "";
    };
  }, []);

  useEffect(() => {
    let mounted = true;
    let timer: number | undefined;
    const syncRuntime = async () => {
      try {
        const runtime = await getRuntimeSettings();
        if (!mounted) return;
        setVisualizerSensitivity(runtime.pillVisualizerSensitivity || 10);
        setActivitySensitivity(runtime.activitySensitivity || 4.2);
        setActivityNoiseGate(runtime.activityNoiseGate ?? 0.0015);
        setActivityClipThreshold(runtime.activityClipThreshold ?? 0.32);
      } catch {
        // no-op
      }
    };
    void syncRuntime();
    timer = window.setInterval(() => void syncRuntime(), 2500);
    return () => {
      mounted = false;
      if (timer) window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listenTranscript((event) => {
      const latestFinal = [...event.segments]
        .reverse()
        .find((seg) => seg.kind === "final");
      if (!latestFinal) return;
      if (latestFinal.confidence != null) {
        setFinalConfidence(latestFinal.confidence);
      } else {
        const words = latestFinal.text.trim().split(/\s+/).filter(Boolean).length;
        setFinalConfidence(Math.min(0.95, 0.45 + words * 0.04));
      }
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {
        // no-op
      });
    return () => {
      unlisten?.();
    };
  }, []);

  const handleDragPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    void getCurrentWindow().startDragging();
  };

  const handleDragMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    void getCurrentWindow().startDragging();
  };

  const boostedLevel = boostPillLevel(level, visualizerSensitivity);
  const confidenceBand =
    finalConfidence == null ? "unknown" : finalConfidence < 0.55 ? "low" : finalConfidence < 0.75 ? "mid" : "high";

  // Convert raw level (0-1) into a stable width for the pill meter.
  // Keep it responsive even when VAD briefly flips false.
  const visualizerWidth = isActive
    ? Math.min(100, Math.max(8, (isSpeech ? boostedLevel : boostedLevel * 0.72) * 100))
    : 0;

  return (
    <main className="pill-root" data-tauri-drag-region>
      <div
        className={`pill-shell${isActive ? " pill-expanded" : " pill-collapsed"}${isSpeech ? " pill-speaking" : ""}${
          confidenceBand === "low" ? " pill-low-confidence" : confidenceBand === "mid" ? " pill-mid-confidence" : ""
        }${isNoisy ? " pill-noisy" : ""}${isClipping ? " pill-clipping" : ""}`}
        data-tauri-drag-region
        onPointerDown={handleDragPointerDown}
        onMouseDown={handleDragMouseDown}
        role="status"
        aria-label="Dictum status pill. Drag to move. Use Ctrl+Shift+Space to toggle dictation."
      >
        <div className="pill-mask" aria-hidden>
          <div className="pill-stars">
            <span className="pill-star" />
            <span className="pill-star" />
            <span className="pill-star" />
            <span className="pill-star" />
            <span className="pill-star" />
            <span className="pill-star" />
          </div>

          <div className="pill-health" aria-hidden>
            <span className={`pill-health-dot confidence-${confidenceBand}`} />
            <span className={`pill-health-dot noise-${isNoisy ? "on" : "off"}`} />
            <span className={`pill-health-dot clip-${isClipping ? "on" : "off"}`} />
          </div>

          <div className="pill-logo-wrap">
            <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" className="lattice-logo">
              <defs>
                <linearGradient id="lattice-accent-grad" x1="0%" y1="0%" x2="100%" y2="100%">
                  <stop offset="0%" stopColor="rgb(var(--accent))" />
                  <stop offset="50%" stopColor="rgb(var(--accent-glow))" />
                  <stop offset="100%" stopColor="rgb(var(--accent-2))" />
                </linearGradient>
              </defs>
              <path
                d="M12.5 3a9 9 0 1 0 8.5 12.5A7 7 0 0 1 12.5 3z"
                className="logo-moon"
              />
              <circle
                cx="17.5" cy="6.5" r="1.5"
                className="logo-dot"
                style={{ transform: `scale(${isActive ? 1 + level * 0.4 : 1})`, transformOrigin: "17.5px 6.5px" }}
              />
            </svg>
          </div>

          <div className="pill-visualizer-wrap">
            <div className="audio-track">
              <div
                className="audio-fill"
                style={{ width: `${visualizerWidth}%` }}
              />
            </div>
          </div>
        </div>

        <span
          style={{
            position: "absolute",
            width: 1,
            height: 1,
            overflow: "hidden",
            clip: "rect(0 0 0 0)",
            whiteSpace: "nowrap",
          }}
          role="status"
          aria-live="polite"
        >
          {status}
        </span>
      </div>
    </main>
  );
}
