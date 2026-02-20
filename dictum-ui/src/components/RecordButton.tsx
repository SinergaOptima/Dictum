"use client";

interface RecordButtonProps {
  isListening: boolean;
  onToggle: () => void;
}

/**
 * Primary action button — toggles capture on/off.
 *
 * - Idle / Stopped → green "Start" button
 * - Listening → red "Stop" button with pulsing ring
 */
export function RecordButton({ isListening, onToggle }: RecordButtonProps) {
  return (
    <button
      onClick={onToggle}
      className={`
        relative flex min-w-[200px] items-center justify-center gap-2
        rounded-full px-8 py-3 text-sm font-semibold
        transition-all duration-200 focus:outline-none focus-visible:ring-2
        focus-visible:ring-dictum-accent/60 focus-visible:ring-offset-2
        focus-visible:ring-offset-dictum-surface
        ${
          isListening
            ? "bg-dictum-stopped hover:bg-dictum-stopped/90 text-white shadow-floating"
            : "bg-dictum-accent hover:bg-dictum-accent-hover text-white shadow-floating"
        }
      `}
      aria-label={isListening ? "Stop recording" : "Start recording"}
    >
      {/* Pulsing outer ring while listening */}
      {isListening && (
        <span
          className="absolute inset-0 animate-ping rounded-full bg-dictum-stopped/40"
          aria-hidden
        />
      )}

      {/* Mic / stop icon */}
      <span
        aria-hidden
        className={`h-2.5 w-2.5 rounded-full ${isListening ? "bg-white" : "bg-dictum-warm"}`}
      >
      </span>

      {isListening ? "Stop Capture" : "Start Capture"}
    </button>
  );
}
