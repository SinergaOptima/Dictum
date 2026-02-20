"use client";

import type { TranscriptSegment } from "@shared/ipc_types";

interface TranscriptFeedProps {
  segments: TranscriptSegment[];
}

/**
 * Renders the live transcript.
 *
 * - **Partial** segments are displayed in a dimmed italic style to signal
 *   that they may change on the next update.
 * - **Final** segments are rendered in full, committed style.
 * - Segments with the same `id` replace each other (partial → final dedup).
 */
export function TranscriptFeed({ segments }: TranscriptFeedProps) {
  const confidenceLabel = (confidence: number | null) => {
    if (confidence == null) {
      return null;
    }
    return `${Math.round(confidence * 100)}%`;
  };

  return (
    <div className="space-y-3">
      {segments.map((seg) => {
        const confidence = confidenceLabel(seg.confidence);
        return (
          <div
            key={seg.id}
            className={`animate-fade-in rounded-md border px-4 py-3 font-mono text-sm leading-relaxed shadow-flush transition-all duration-150 ${
              seg.kind === "partial"
                ? "border-dictum-border/70 bg-dictum-bg/50 text-dictum-partial italic opacity-90"
                : "border-dictum-accent/25 bg-dictum-surface text-dictum-final"
            }`}
          >
            <div className="mb-1 flex items-center justify-between gap-2 text-[10px] uppercase tracking-[0.16em] text-dictum-muted/80">
              <span>{seg.kind}</span>
              {confidence && (
                <span className="rounded-full border border-dictum-cool/30 bg-dictum-cool/10 px-2 py-0.5 text-[9px] tracking-[0.12em] text-dictum-cool">
                  conf {confidence}
                </span>
              )}
            </div>
            {seg.text}
          </div>
        );
      })}
    </div>
  );
}
