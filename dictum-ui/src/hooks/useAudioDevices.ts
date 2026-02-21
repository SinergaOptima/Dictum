"use client";

import { useCallback, useEffect, useState } from "react";
import { listAudioDevices } from "@/lib/tauri";
import type { DeviceInfo } from "@shared/ipc_types";

interface UseAudioDevicesResult {
  devices: DeviceInfo[];
  defaultDevice: DeviceInfo | null;
  recommendedDevice: DeviceInfo | null;
  loading: boolean;
  error: string | null;
  refreshDevices: () => Promise<void>;
}

export function useAudioDevices(): UseAudioDevicesResult {
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refreshDevices = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await listAudioDevices();
      setDevices(list);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshDevices();
  }, [refreshDevices]);

  return {
    devices,
    defaultDevice: devices.find((d) => d.isDefault) ?? null,
    recommendedDevice: devices.find((d) => d.isRecommended) ?? null,
    loading,
    error,
    refreshDevices,
  };
}
