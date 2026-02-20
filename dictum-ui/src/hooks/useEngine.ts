"use client";

import { useCallback, useEffect, useState } from "react";
import {
  getStatus,
  listenStatus,
  startEngine as apiStartEngine,
  stopEngine as apiStopEngine,
} from "@/lib/tauri";
import type { EngineStatus } from "@shared/ipc_types";

interface UseEngineResult {
  status: EngineStatus;
  isListening: boolean;
  error: string | null;
  startEngine: (deviceName?: string | null) => Promise<void>;
  stopEngine: () => Promise<void>;
}

/**
 * React hook for controlling the Dictum engine.
 *
 * Subscribes to `dictum://status` events so the UI stays in sync with
 * backend state transitions (e.g. automatic stop on error).
 *
 * ## Usage
 * ```tsx
 * const { isListening, startEngine, stopEngine } = useEngine();
 * ```
 */
export function useEngine(): UseEngineResult {
  const [status, setStatus] = useState<EngineStatus>("idle");
  const [error, setError] = useState<string | null>(null);

  // Fetch the initial status when the component mounts
  useEffect(() => {
    getStatus()
      .then(setStatus)
      .catch((err) => console.warn("Could not fetch initial status:", err));
  }, []);

  // Subscribe to live status updates from the engine
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listenStatus((event) => {
      setStatus(event.status);
      if (event.detail) {
        setError(event.detail);
      } else {
        setError(null);
      }
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => {
        console.error("Failed to subscribe to status events:", err);
      });

    return () => {
      unlisten?.();
    };
  }, []);

  const startEngine = useCallback(async (deviceName?: string | null) => {
    setError(null);
    try {
      await apiStartEngine(deviceName ?? null);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg);
    }
  }, []);

  const stopEngine = useCallback(async () => {
    setError(null);
    try {
      await apiStopEngine();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg);
    }
  }, []);

  return {
    status,
    isListening: status === "listening",
    error,
    startEngine,
    stopEngine,
  };
}
