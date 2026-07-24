// src/hooks/useArgus.ts
//
// Typed wrapper around the four Argus Tauri commands invoked from
// src/panels/privacy/Argus*Section.tsx. The Free-side commands
// (commander-free/src/argus.rs) forward opaque serde_json::Value from
// the Pro sidecar rather than a ts-rs-derived struct, so the response
// shapes below are hand-typed to match the Pro collectors' JSON — there
// is no generated binding to import for these IPC calls.
//
// PRIVACY INVARIANT (see AGENTS.md): none of these shapes carry window
// titles, exe paths, URLs, filenames, printer names, document names, or
// usernames — only aggregate scalars/enums cross the IPC boundary.
//
// Exposed as a plain module-level object (not a React hook with
// per-render identity) since every entry is a stateless IPC call with no
// captured props/state — this keeps `argus` a stable dependency for
// callers' effects/callbacks.

import { invoke } from "@tauri-apps/api/core";

export interface ArgusCollectorStatus {
  running: boolean;
  startedAt: string | null;
}

/** App-usage monitor status. Also carries the poll interval. */
export interface ArgusAppUsageStatus extends ArgusCollectorStatus {
  intervalMs: number | null;
}

/** One window slot in the app-usage recent-windows view.
 *  INVARIANT: no exe path or window title — only aggregate data. */
export interface ArgusWindowSlot {
  windowStart: string;
  windowEnd: string;
  activeSeconds: number;
  idleSeconds: number;
  topCategory: string;
  categoryScores: Record<string, number>;
}

/** One aggregate Argus signal record (DLP / tamper / print-USB).
 *  INVARIANT: no filenames, paths, URLs, or usernames — only
 *  kind/class/magnitude/severity. */
export interface ArgusSignalEntry {
  windowStart: string;
  windowEnd: string;
  kind: string;
  class: string;
  magnitude: number;
  severity: string;
}

/** Last productivity summary the fleet received (aggregate scalars only). */
export interface MirrorProductivity {
  windowStart: string;
  windowEnd: string;
  activeSeconds: number;
  idleSeconds: number;
  categoryScores: Record<string, number>;
}

/** Non-draining peek of what the fleet has queued and last sent.
 *  INVARIANT: aggregate scalars only — no names/paths/URLs/usernames. */
export interface MonitoringMirror {
  pendingSignals: ArgusSignalEntry[];
  lastProductivity: MirrorProductivity | null;
}

async function optionalRecent<T>(command: string): Promise<T[]> {
  const result = await invoke<T[]>(command).catch(() => [] as T[]);
  return Array.isArray(result) ? result : [];
}

/** Typed Argus IPC client — import as `argus` (not a hook; plain object). */
export const argus = {
  // ── App-usage monitor ────────────────────────────────────────────────
  appUsageStart: () => invoke("argus_app_usage_start", { args: {} }),
  appUsageStop: () => invoke("argus_app_usage_stop"),
  appUsageStatus: () => invoke<ArgusAppUsageStatus>("argus_app_usage_status"),
  appUsageRecent: () => optionalRecent<ArgusWindowSlot>("argus_app_usage_recent"),

  // ── DLP signal monitor ───────────────────────────────────────────────
  dlpStart: () => invoke("argus_dlp_start", { args: {} }),
  dlpStop: () => invoke("argus_dlp_stop"),
  dlpStatus: () => invoke<ArgusCollectorStatus>("argus_dlp_status"),
  dlpRecent: () => optionalRecent<ArgusSignalEntry>("argus_dlp_recent"),

  // ── Tamper detector ──────────────────────────────────────────────────
  tamperStart: () => invoke("argus_tamper_start", { args: {} }),
  tamperStop: () => invoke("argus_tamper_stop"),
  tamperStatus: () => invoke<ArgusCollectorStatus>("argus_tamper_status"),
  tamperRecent: () => optionalRecent<ArgusSignalEntry>("argus_tamper_recent"),

  // ── Print & removable-media monitor ─────────────────────────────────
  printUsbStart: () => invoke("argus_print_usb_start", { args: {} }),
  printUsbStop: () => invoke("argus_print_usb_stop"),
  printUsbStatus: () => invoke<ArgusCollectorStatus>("argus_print_usb_status"),
  printUsbRecent: () => optionalRecent<ArgusSignalEntry>("argus_print_usb_recent"),

  // ── Monitoring mirror ("what my employer sees") ──────────────────────
  // Non-draining peek of pending argus signals + last-sent productivity
  // summary. Forwarded opaque from fleet_push::monitoring_mirror.
  monitoringMirror: () => invoke<MonitoringMirror>("argus_monitoring_mirror"),
};

export default argus;
