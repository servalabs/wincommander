import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/bp";

type DiagnosticEvent = {
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

type DiagnosticsHealth = {
  persistedEvents: number;
  droppedEvents: number;
  redactedFields: number;
  healthy: boolean;
};

function readable(value: string): string {
  return value.replaceAll("_", " ");
}

function timestamp(value: string): string {
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? "Unknown time" : new Date(parsed).toLocaleString();
}

export default function SupportConsole() {
  const [events, setEvents] = useState<DiagnosticEvent[]>([]);
  const [health, setHealth] = useState<DiagnosticsHealth | null>(null);
  const [loading, setLoading] = useState(true);
  const [unavailable, setUnavailable] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextEvents, nextHealth, serviceEvents] = await Promise.all([
        invoke<DiagnosticEvent[]>("get_diagnostic_events", { limit: 50 }),
        invoke<DiagnosticsHealth>("get_diagnostics_health"),
        invoke<DiagnosticEvent[]>("get_service_diagnostic_summaries", { limit: 50 }).catch(() => []),
      ]);
      const combined = [...nextEvents, ...serviceEvents.map(event => ({ ...event, lifecycle: "applied", source: "service" as const }))]
        .sort((left, right) => right.occurredAt.localeCompare(left.occurredAt));
      setEvents(combined);
      setHealth(nextHealth);
      setUnavailable(false);
    } catch {
      setEvents([]);
      setHealth(null);
      setUnavailable(true);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  if (unavailable) {
    return (
      <div className="text-[12px] leading-5 text-[var(--text-dim)]">
        Support Console is unavailable in this installed build. Update WinCommander to view structured diagnostics.
        <div className="mt-3"><Button text="Refresh" icon="refresh" className="compact-action-btn" loading={loading} onClick={refresh} /></div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="m-0 max-w-2xl text-[12px] leading-5 text-[var(--text-dim)]">
          Safe operation history. This view never displays files, passwords, clipboard content, camera data, or raw system errors.
        </p>
        <Button text="Refresh" icon="refresh" className="compact-action-btn" loading={loading} onClick={refresh} />
      </div>

      {health && (
        <div className="grid grid-cols-2 gap-2 md:grid-cols-4" aria-label="Diagnostics health">
          <Metric label="Store" value={health.healthy ? "Healthy" : "Needs attention"} />
          <Metric label="Stored" value={String(health.persistedEvents)} />
          <Metric label="Dropped" value={String(health.droppedEvents)} />
          <Metric label="Redacted" value={String(health.redactedFields)} />
        </div>
      )}

      {!loading && events.length === 0 && (
        <p className="m-0 text-[12px] text-[var(--text-mute)]">No structured diagnostic events have been recorded yet.</p>
      )}

      <div className="flex max-h-[420px] flex-col gap-2 overflow-y-auto pr-1" aria-live="polite">
        {events.map((event) => (
          <article key={event.eventId} className="rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3">
            <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
              <span className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">
                {event.operationId}
              </span>
              <span className="text-[10px] text-[var(--text-mute)]">{timestamp(event.occurredAt)}</span>
            </div>
            <div className="mt-1 text-[13px] font-medium text-[var(--text)]">
              {readable(event.feature)} · {readable(event.action)} · {readable(event.stage)}
            </div>
            <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-[var(--text-dim)]">
              <span>{readable(event.lifecycle)} → {readable(event.outcome)}</span>
              {event.source === "service" && <span>Service record</span>}
              <span>Severity: {readable(event.severity)}</span>
              <span>Retry: {readable(event.retryability)}</span>
              {event.durationMs !== undefined && <span>{event.durationMs} ms</span>}
              {event.errorCode && <span>Code: {event.errorCode}</span>}
            </div>
            <div className="mt-1 text-[11px] text-[var(--text-mute)]">Next: {readable(event.suggestedNextAction)}</div>
          </article>
        ))}
      </div>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-[var(--r-sm)] border border-[var(--border)] bg-[var(--surface-2)] px-2.5 py-2">
      <div className="font-[family-name:var(--font-mono)] text-[9px] uppercase tracking-wider text-[var(--text-mute)]">{label}</div>
      <div className="mt-0.5 text-[12px] font-medium text-[var(--text)]">{value}</div>
    </div>
  );
}
