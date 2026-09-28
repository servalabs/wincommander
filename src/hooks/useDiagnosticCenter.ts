import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { LogRecord } from "../lib/logFilter";
import { DIAGNOSTIC_RECORDED_EVENT } from "../lib/diagnostics";

const LIVE_REFRESH_DEBOUNCE_MS = 150;

export type DiagnosticEvent = {
  eventId: string;
  operationId: string;
  occurredAt: string;
  feature: string;
  action: string;
  stage: string;
  lifecycle: string;
  outcome: string;
  errorCode?: string;
  severity: string;
  retryability: string;
  suggestedNextAction: string;
  durationMs?: number;
  source?: "desktop" | "service" | "pro";
};

export type DiagnosticsHealth = {
  persistedEvents: number;
  droppedEvents: number;
  redactedFields: number;
  healthy: boolean;
};

export function useDiagnosticCenter() {
  const [structuredEvents, setStructuredEvents] = useState<DiagnosticEvent[]>([]);
  const [legacyRecords, setLegacyRecords] = useState<LogRecord[]>([]);
  const [health, setHealth] = useState<DiagnosticsHealth | null>(null);
  const [loading, setLoading] = useState(true);
  const [unavailable, setUnavailable] = useState(false);
  const refreshInFlightRef = useRef(false);
  const refreshPendingRef = useRef(false);

  const refresh = useCallback(async () => {
    if (refreshInFlightRef.current) {
      // A write can finish while the initial screen load is still reading.
      // Queue one more pass so the just-written event is not missed.
      refreshPendingRef.current = true;
      return;
    }
    refreshInFlightRef.current = true;
    setLoading(true);
    try {
      const [nextEvents, nextHealth, serviceEvents, proEvents, nextLegacyRecords] = await Promise.all([
        invoke<DiagnosticEvent[]>("get_diagnostic_events", { limit: 500 }).catch(() => null),
        invoke<DiagnosticsHealth>("get_diagnostics_health").catch(() => null),
        invoke<DiagnosticEvent[]>("get_service_diagnostic_summaries", { limit: 500 }).catch(() => []),
        invoke<DiagnosticEvent[]>("get_pro_diagnostic_summaries", { limit: 500 }).catch(() => []),
        invoke<LogRecord[]>("get_log_records", { limit: 500, levels: null }).catch(() => null),
      ]);
      if (nextEvents === null && nextLegacyRecords === null) throw new Error("No diagnostic source is available");
      setStructuredEvents([
        ...(nextEvents ?? []).map((event) => ({ ...event, source: "desktop" as const })),
        ...serviceEvents.map((event) => ({ ...event, source: "service" as const })),
        ...proEvents.map((event) => ({ ...event, source: "pro" as const })),
      ]);
      setLegacyRecords(nextLegacyRecords ?? []);
      setHealth(nextHealth);
      setUnavailable(false);
    } catch {
      setStructuredEvents([]);
      setLegacyRecords([]);
      setHealth(null);
      setUnavailable(true);
    } finally {
      setLoading(false);
      refreshInFlightRef.current = false;
      if (refreshPendingRef.current) {
        refreshPendingRef.current = false;
        void refresh();
      }
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    let scheduledRefresh: number | undefined;
    const refreshAfterDurableWrite = () => {
      if (scheduledRefresh !== undefined) return;
      scheduledRefresh = window.setTimeout(() => {
        scheduledRefresh = undefined;
        void refresh();
      }, LIVE_REFRESH_DEBOUNCE_MS);
    };
    // A native writer does not have a WebView event channel. Refresh when the
    // user returns to this screen instead of repeatedly reading all five
    // diagnostic sources while it remains open.
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") refreshAfterDurableWrite();
    };
    window.addEventListener(DIAGNOSTIC_RECORDED_EVENT, refreshAfterDurableWrite);
    window.addEventListener("focus", refreshWhenVisible);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.removeEventListener(DIAGNOSTIC_RECORDED_EVENT, refreshAfterDurableWrite);
      window.removeEventListener("focus", refreshWhenVisible);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
      if (scheduledRefresh !== undefined) window.clearTimeout(scheduledRefresh);
    };
  }, [refresh]);

  return { structuredEvents, legacyRecords, health, loading, unavailable, refresh };
}
