"use client";

import { useCallback, useEffect, useState } from "react";
import { listenTranscript } from "@/lib/tauri";
import type { TranscriptSegment } from "@shared/ipc_types";

/**
 * React hook that subscribes to `dictum://transcript` events and maintains
 * a deduplicated, ordered list of segments.
 *
 * ## Deduplication
 * Segments with the same `id` replace each other — this handles the
 * partial → final transition where the server emits multiple events for
 * the same utterance.
 *
 * ## Usage
 * ```tsx
 * const { segments } = useTranscript();
 * ```
 */
export function useTranscript() {
  // Map from utterance id → segment for O(1) dedup
  const [segmentMap, setSegmentMap] = useState<Map<string, TranscriptSegment>>(
    new Map()
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listenTranscript((event) => {
      setSegmentMap((prev) => {
        const next = new Map(prev);
        for (const seg of event.segments) {
          next.set(seg.id, seg);
        }
        return next;
      });
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => {
        console.error("Failed to subscribe to transcript events:", err);
      });

    return () => {
      unlisten?.();
    };
  }, []);

  // Convert map to stable ordered array (insertion order = event order)
  const segments = Array.from(segmentMap.values());

  const clearSegments = useCallback(() => {
    setSegmentMap(new Map());
  }, []);

  return { segments, clearSegments };
}
