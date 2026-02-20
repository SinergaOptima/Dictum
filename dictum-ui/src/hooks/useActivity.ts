"use client";

import { useEffect, useRef, useState } from "react";
import { listenActivity } from "@/lib/tauri";

interface UseActivityResult {
  level: number;
  isSpeech: boolean;
}

const clamp01 = (value: number): number => Math.min(1, Math.max(0, value));

export function useActivity(): UseActivityResult {
  const [level, setLevel] = useState(0);
  const [isSpeech, setIsSpeech] = useState(false);
  const lastSpeechTs = useRef(0);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listenActivity((event) => {
      const nextLevel = clamp01(event.rms * 4.2);
      setLevel((prev) => clamp01(Math.max(nextLevel, prev * 0.78)));
      setIsSpeech(event.isSpeech);
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
      setLevel((prev) => clamp01(prev * 0.9));
      if (Date.now() - lastSpeechTs.current > 220) {
        setIsSpeech(false);
      }
    }, 60);

    return () => window.clearInterval(timer);
  }, []);

  return { level, isSpeech };
}
