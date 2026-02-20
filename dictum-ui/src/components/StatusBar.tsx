"use client";

import type { EngineStatus } from "@shared/ipc_types";

interface StatusBarProps {
  status: EngineStatus;
}

const STATUS_CONFIG: Record<
  EngineStatus,
  { label: string; dotClass: string; panelClass: string }
> = {
  idle: {
    label: "idle",
    dotClass: "bg-dictum-idle",
    panelClass: "border-dictum-border/80 bg-dictum-bg/45",
  },
  warmingup: {
    label: "warming up",
    dotClass: "bg-dictum-warm animate-pulse",
    panelClass: "border-dictum-warm/40 bg-dictum-warm/10",
  },
  listening: {
    label: "listening",
    dotClass: "bg-dictum-listening animate-pulse",
    panelClass: "border-dictum-listening/40 bg-dictum-listening/10",
  },
  stopped: {
    label: "stopped",
    dotClass: "bg-dictum-stopped",
    panelClass: "border-dictum-stopped/40 bg-dictum-stopped/10",
  },
  error: {
    label: "error",
    dotClass: "bg-dictum-stopped",
    panelClass: "border-dictum-stopped/45 bg-dictum-stopped/10",
  },
};

/**
 * Small status indicator in the top-right of the header.
 * Shows a coloured dot + label reflecting the engine state.
 */
export function StatusBar({ status }: StatusBarProps) {
  const cfg = STATUS_CONFIG[status] ?? STATUS_CONFIG.idle;

  return (
    <div
      className={`flex items-center gap-2 rounded-full border px-3 py-1.5 select-none ${cfg.panelClass}`}
    >
      <span
        className={`inline-block h-2.5 w-2.5 rounded-full ${cfg.dotClass}`}
        aria-hidden
      />
      <span className="text-[11px] font-semibold uppercase tracking-[0.14em] text-dictum-muted">
        {cfg.label}
      </span>
    </div>
  );
}
