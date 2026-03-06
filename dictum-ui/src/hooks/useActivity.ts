"use client";

import { useEffect, useRef, useState } from "react";
import { listenActivity } from "@/lib/tauri";

interface UseActivityResult {
  level: number;
  isSpeech: boolean;
  rawRms: number;
  isNoisy: boolean;
  isClipping: boolean;
}

interface UseActivityOptions {
  sensitivity?: number;
  noiseGate?: number;
  clipThreshold?: number;
}

const clamp01 = (value: number): number => Math.min(1, Math.max(0, value));
const ACTIVITY_UI_INTERVAL_MS = 50;

type ActivityState = UseActivityResult;

const sameActivityState = (a: ActivityState, b: ActivityState): boolean =>
  Math.abs(a.level - b.level) < 0.002 &&
  Math.abs(a.rawRms - b.rawRms) < 0.0002 &&
  a.isSpeech === b.isSpeech &&
  a.isNoisy === b.isNoisy &&
  a.isClipping === b.isClipping;

export function useActivity(options?: UseActivityOptions): UseActivityResult {
  const settingsRef = useRef({
    sensitivity: options?.sensitivity ?? 4.2,
    noiseGate: options?.noiseGate ?? 0.0015,
    clipThreshold: options?.clipThreshold ?? 0.32,
  });
  const latestStateRef = useRef<ActivityState>({
    level: 0,
    isSpeech: false,
    rawRms: 0,
    isNoisy: false,
    isClipping: false,
  });
  const [activity, setActivity] = useState<ActivityState>(latestStateRef.current);
  const lastSpeechTs = useRef(0);

  useEffect(() => {
    settingsRef.current = {
      sensitivity: options?.sensitivity ?? 4.2,
      noiseGate: options?.noiseGate ?? 0.0015,
      clipThreshold: options?.clipThreshold ?? 0.32,
    };
  }, [options?.clipThreshold, options?.noiseGate, options?.sensitivity]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listenActivity((event) => {
      const { sensitivity, noiseGate, clipThreshold } = settingsRef.current;
      const gated = Math.max(0, event.rms - noiseGate);
      const nextLevel = clamp01(gated * sensitivity);
      latestStateRef.current = {
        level: clamp01(Math.max(nextLevel, latestStateRef.current.level * 0.78)),
        rawRms: event.rms,
        isSpeech: event.isSpeech,
        isNoisy: !event.isSpeech && event.rms > noiseGate * 1.2,
        isClipping: event.rms >= clipThreshold,
      };
      if (event.isSpeech) {
        lastSpeechTs.current = Date.now();
      }
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => {
        console.error("Failed to subscribe to activity events:", err);
      });

    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      const next: ActivityState = {
        ...latestStateRef.current,
        level: clamp01(latestStateRef.current.level * 0.9),
        isSpeech:
          Date.now() - lastSpeechTs.current <= 220 && latestStateRef.current.isSpeech,
      };
      latestStateRef.current = next;
      setActivity((prev) => (sameActivityState(prev, next) ? prev : next));
    }, ACTIVITY_UI_INTERVAL_MS);

    return () => window.clearInterval(timer);
  }, []);

  return activity;
}
