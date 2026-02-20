"use client";

import { useEffect, type MouseEvent, type PointerEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useActivity } from "@/hooks/useActivity";
import { useEngine } from "@/hooks/useEngine";

const BAR_HEIGHTS = Array.from({ length: 17 }, (_, i) => {
  const center = 8;
  const distance = Math.abs(i - center) / center;
  return 0.34 + (1 - distance) * 0.66;
});

export default function PillPage() {
  const { isListening, status } = useEngine();
  const { isSpeech, level } = useActivity();
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

  const handleDragPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    void getCurrentWindow().startDragging();
  };

  const handleDragMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    void getCurrentWindow().startDragging();
  };

  return (
    <main className="pill-root" data-tauri-drag-region>
      <div
        className={`pill-shell${isActive ? " pill-expanded" : " pill-collapsed"}${isListening ? " pill-listening" : ""}${isSpeech ? " pill-speaking" : ""}`}
        data-tauri-drag-region
        onPointerDown={handleDragPointerDown}
        onMouseDown={handleDragMouseDown}
        role="status"
        aria-label="Dictum status pill. Drag to move. Use Ctrl+Shift+Space to toggle dictation."
      >
        {isActive ? (
          <div className="pill-bars" aria-hidden>
            {BAR_HEIGHTS.map((base, i) => {
              const maxBar = 15;
              const floor = 4;
              const responsiveness = isSpeech
                ? 0.44 + level * 1.1
                : 0.2 + level * 0.34;
              const height = Math.min(
                maxBar,
                floor + base * (maxBar - floor) * responsiveness,
              );
              return (
                <span
                  key={i}
                  className="pill-bar"
                  style={{ height: `${Math.max(2, height)}px` }}
                />
              );
            })}
          </div>
        ) : null}
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
