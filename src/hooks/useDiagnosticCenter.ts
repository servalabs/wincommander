import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { LogRecord } from "../lib/logFilter";

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
  source?: "desktop" | "service";
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

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextEvents, nextHealth, serviceEvents, nextLegacyRecords] = await Promise.all([
        invoke<DiagnosticEvent[]>("get_diagnostic_events", { limit: 500 }).catch(() => null),
        invoke<DiagnosticsHealth>("get_diagnostics_health").catch(() => null),
        invoke<DiagnosticEvent[]>("get_service_diagnostic_summaries", { limit: 500 }).catch(() => []),
        invoke<LogRecord[]>("get_log_records", { limit: 500, levels: null }).catch(() => null),
      ]);
      if (nextEvents === null && nextLegacyRecords === null) throw new Error("No diagnostic source is available");
      setStructuredEvents([
        ...(nextEvents ?? []).map((event) => ({ ...event, source: "desktop" as const })),
        ...serviceEvents.map((event) => ({ ...event, source: "service" as const, lifecycle: "applied" })),
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
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  return { structuredEvents, legacyRecords, health, loading, unavailable, refresh };
}
