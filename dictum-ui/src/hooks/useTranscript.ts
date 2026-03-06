"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { listenTranscript } from "@/lib/tauri";
import type { TranscriptSegment } from "@shared/ipc_types";

type SegmentEnvelope = TranscriptSegment & { receivedAt: number };

const UNSTABLE_TAIL_MS = 6500;
const MAX_TAIL_SEGMENTS = 2;
const TRANSCRIPT_FLUSH_DELAY_MS = 48;

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
  const segmentMapRef = useRef<Map<string, SegmentEnvelope>>(new Map());
  const flushTimerRef = useRef<number | null>(null);
  const [segments, setSegments] = useState<TranscriptSegment[]>([]);

  const flushSegments = useCallback(() => {
    flushTimerRef.current = null;
    const next = Array.from(segmentMapRef.current.values()).map(
      ({ receivedAt: _receivedAt, ...seg }) => seg,
    );
    setSegments((prev) => {
      if (prev.length !== next.length) return next;
      for (let i = 0; i < prev.length; i += 1) {
        const a = prev[i];
        const b = next[i];
        if (
          !a ||
          !b ||
          a.id !== b.id ||
          a.text !== b.text ||
          a.kind !== b.kind ||
          a.confidence !== b.confidence
        ) {
          return next;
        }
      }
      return prev;
    });
  }, []);

  const scheduleFlush = useCallback(() => {
    if (flushTimerRef.current !== null) return;
    flushTimerRef.current = window.setTimeout(flushSegments, TRANSCRIPT_FLUSH_DELAY_MS);
  }, [flushSegments]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listenTranscript((event) => {
      const next = new Map(segmentMapRef.current);
      const now = Date.now();
      let didChange = false;
      for (const seg of event.segments) {
        if (seg.kind === "final") {
          const finalTail = Array.from(next.entries())
            .filter(([, existing]) => existing.kind === "final" && now - existing.receivedAt <= UNSTABLE_TAIL_MS)
            .slice(-MAX_TAIL_SEGMENTS);
          if (finalTail.length > 0) {
            const tailText = finalTail.map(([, existing]) => existing.text).join(" ").trim();
            if (shouldRewriteTail(tailText, seg.text)) {
              for (const [tailId] of finalTail) {
                didChange = next.delete(tailId) || didChange;
              }
            }
          }
        }
        const previous = next.get(seg.id);
        if (
          !previous ||
          previous.text !== seg.text ||
          previous.kind !== seg.kind ||
          previous.confidence !== seg.confidence
        ) {
          didChange = true;
        }
        next.set(seg.id, { ...seg, receivedAt: now });
      }
      segmentMapRef.current = next;
      if (!didChange) return;
      scheduleFlush();
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => {
        console.error("Failed to subscribe to transcript events:", err);
      });

    return () => {
      if (flushTimerRef.current !== null) {
        window.clearTimeout(flushTimerRef.current);
        flushTimerRef.current = null;
      }
      unlisten?.();
    };
  }, [scheduleFlush]);

  const clearSegments = useCallback(() => {
    if (flushTimerRef.current !== null) {
      window.clearTimeout(flushTimerRef.current);
      flushTimerRef.current = null;
    }
    segmentMapRef.current = new Map();
    setSegments([]);
  }, []);

  return { segments, clearSegments };
}
