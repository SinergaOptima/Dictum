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

export function useActivity(options?: UseActivityOptions): UseActivityResult {
  const sensitivity = options?.sensitivity ?? 4.2;
  const noiseGate = options?.noiseGate ?? 0.0015;
  const clipThreshold = options?.clipThreshold ?? 0.32;
  const [level, setLevel] = useState(0);
  const [isSpeech, setIsSpeech] = useState(false);
  const [rawRms, setRawRms] = useState(0);
  const [isNoisy, setIsNoisy] = useState(false);
  const [isClipping, setIsClipping] = useState(false);
  const lastSpeechTs = useRef(0);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listenActivity((event) => {
      const gated = Math.max(0, event.rms - noiseGate);
      const nextLevel = clamp01(gated * sensitivity);
      setLevel((prev) => clamp01(Math.max(nextLevel, prev * 0.78)));
      setRawRms(event.rms);
      setIsSpeech(event.isSpeech);
      setIsNoisy(!event.isSpeech && event.rms > noiseGate * 1.2);
      setIsClipping(event.rms >= clipThreshold);
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
  }, [clipThreshold, noiseGate, sensitivity]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setLevel((prev) => clamp01(prev * 0.9));
      if (Date.now() - lastSpeechTs.current > 220) {
        setIsSpeech(false);
      }
    }, 60);

    return () => window.clearInterval(timer);
  }, []);

  return { level, isSpeech, rawRms, isNoisy, isClipping };
}
