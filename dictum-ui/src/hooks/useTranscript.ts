"use client";

import { useCallback, useEffect, useState } from "react";
import { listenTranscript } from "@/lib/tauri";
import type { TranscriptSegment } from "@shared/ipc_types";

type SegmentEnvelope = TranscriptSegment & { receivedAt: number };

const UNSTABLE_TAIL_MS = 6500;
const MAX_TAIL_SEGMENTS = 2;

const tokenize = (text: string): string[] =>
  text.toLowerCase().match(/[a-z0-9']+/g) ?? [];

const sharedPrefixTokenCount = (a: string[], b: string[]): number => {
  let count = 0;
  const limit = Math.min(a.length, b.length);
  for (let i = 0; i < limit; i += 1) {
    if (a[i] !== b[i]) break;
    count += 1;
  }
  return count;
};

const tokenOverlapRatio = (a: string[], b: string[]): number => {
  if (a.length === 0 || b.length === 0) return 0;
  const aset = new Set(a);
  const bset = new Set(b);
  let common = 0;
  aset.forEach((token) => {
    if (bset.has(token)) common += 1;
  });
  return common / Math.max(1, Math.min(aset.size, bset.size));
};

const shouldRewriteTail = (existingTail: string, incomingFinal: string): boolean => {
  const existing = existingTail.trim();
  const incoming = incomingFinal.trim();
  if (!existing || !incoming) return false;
  if (existing.toLowerCase() === incoming.toLowerCase()) return false;

  const a = tokenize(existing);
  const b = tokenize(incoming);
  if (a.length < 4 || b.length < 4) return false;

  const prefix = sharedPrefixTokenCount(a, b);
  const overlap = tokenOverlapRatio(a, b);
  const startsLikeRevision = prefix >= Math.min(6, Math.max(3, Math.min(a.length, b.length) - 1));
  const strongOverlap = overlap >= 0.62 && prefix >= 2;
  const containment = incoming.toLowerCase().includes(existing.toLowerCase()) ||
    existing.toLowerCase().includes(incoming.toLowerCase());

  return startsLikeRevision || strongOverlap || (containment && prefix >= 2);
};

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
  const [segmentMap, setSegmentMap] = useState<Map<string, SegmentEnvelope>>(
    new Map()
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listenTranscript((event) => {
      setSegmentMap((prev) => {
        const next = new Map(prev);
        const now = Date.now();
        for (const seg of event.segments) {
          if (seg.kind === "final") {
            const finalTail = Array.from(next.entries())
              .filter(([, existing]) => existing.kind === "final" && now - existing.receivedAt <= UNSTABLE_TAIL_MS)
              .slice(-MAX_TAIL_SEGMENTS);
            if (finalTail.length > 0) {
              const tailText = finalTail.map(([, existing]) => existing.text).join(" ").trim();
              if (shouldRewriteTail(tailText, seg.text)) {
                for (const [tailId] of finalTail) {
                  next.delete(tailId);
                }
              }
            }
          }
          next.set(seg.id, { ...seg, receivedAt: now });
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
  const segments = Array.from(segmentMap.values()).map(({ receivedAt: _receivedAt, ...seg }) => seg);

  const clearSegments = useCallback(() => {
    setSegmentMap(new Map());
  }, []);

  return { segments, clearSegments };
}
