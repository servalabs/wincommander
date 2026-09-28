import { useCallback, useMemo, useState } from "react";
import { Button } from "@/components/ui/bp";
import { safeLegacyDiagnosticSource, sanitizeLegacyDiagnosticMessage } from "../../lib/diagnosticSanitizer";
import { useDiagnosticCenter, type DiagnosticEvent } from "../../hooks/useDiagnosticCenter";
import { showError, showSuccess } from "../../utils/toast";

type LegacyRecord = { date: string; timestamp: string; level: string; source: string; message: string };

type SourceFilter = "all" | "desktop" | "service" | "pro" | "legacy";
type SeverityFilter = "all" | "error" | "warn" | "info";

type TimelineEvent = {
  id: string;
  occurredAt: string;
  source: "desktop" | "service" | "pro" | "legacy";
  sourceDetail?: string;
  severity: "error" | "warn" | "info";
  summary: string;
  operationId?: string;
  detail?: string;
  errorCode?: string;
  nextAction?: string;
};

const SOURCE_FILTERS: Array<{ value: SourceFilter; label: string }> = [
  { value: "all", label: "All sources" },
  { value: "desktop", label: "Desktop" },
  { value: "service", label: "Service" },
  { value: "pro", label: "Pro" },
  { value: "legacy", label: "Legacy" },
];

const SEVERITY_FILTERS: Array<{ value: SeverityFilter; label: string }> = [
  { value: "all", label: "All severity" },
  { value: "error", label: "Errors" },
  { value: "warn", label: "Warnings" },
  { value: "info", label: "Information" },
];

function readable(value: string): string {
  return value.replaceAll("_", " ");
}

function timestamp(value: string): string {
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? "Unknown time" : new Date(parsed).toLocaleString();
}

function normalizeSeverity(value: string): TimelineEvent["severity"] {
  const normalized = value.toLowerCase();
  if (normalized === "error" || normalized === "danger" || normalized === "critical") return "error";
  if (normalized === "warn" || normalized === "warning" || normalized === "degraded") return "warn";
  return "info";
}

function legacyTimestamp(record: LegacyRecord): string {
  const parsed = Date.parse(`${record.date}T${record.timestamp}`);
  return Number.isNaN(parsed) ? `${record.date}T${record.timestamp}` : new Date(parsed).toISOString();
}

function structuredTimelineEvent(event: DiagnosticEvent, source: "desktop" | "service" | "pro"): TimelineEvent {
  return {
    id: `${source}:${event.eventId}`,
    occurredAt: event.occurredAt,
    source,
    severity: normalizeSeverity(event.severity),
    summary: `${readable(event.feature)} · ${readable(event.action)} · ${readable(event.stage)}`,
    operationId: event.operationId,
    detail: `${readable(event.lifecycle)} → ${readable(event.outcome)}${event.durationMs === undefined ? "" : ` · ${event.durationMs} ms`}`,
    errorCode: event.errorCode,
    nextAction: readable(event.suggestedNextAction),
  };
}

function legacyTimelineEvent(record: LegacyRecord, index: number): TimelineEvent {
  return {
    id: `legacy:${record.date}:${record.timestamp}:${record.source}:${index}`,
    occurredAt: legacyTimestamp(record),
    source: "legacy",
    sourceDetail: safeLegacyDiagnosticSource(record.source),
    severity: normalizeSeverity(record.level),
    summary: sanitizeLegacyDiagnosticMessage(record.message, record.source, record.level),
  };
}

function copyPlaintext(events: TimelineEvent[]): string {
  return events.map((event) => [
    timestamp(event.occurredAt),
    `[${event.severity.toUpperCase()}]`,
    `[${event.sourceDetail ?? event.source.toUpperCase()}]`,
    event.summary,
    event.operationId ? `operation=${event.operationId}` : "",
    event.errorCode ? `code=${event.errorCode}` : "",
    event.detail ?? "",
    event.nextAction ? `next=${event.nextAction}` : "",
  ].filter(Boolean).join(" ")).join("\n");
}

export default function SupportConsole() {
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>("all");
  const [severityFilter, setSeverityFilter] = useState<SeverityFilter>("all");
  const { structuredEvents, legacyRecords, health, loading, unavailable, refresh } = useDiagnosticCenter();

  const timeline = useMemo(() => [
    ...structuredEvents.map((event) => structuredTimelineEvent(event, event.source ?? "desktop")),
    ...legacyRecords.map(legacyTimelineEvent),
  ].sort((left, right) => right.occurredAt.localeCompare(left.occurredAt)), [legacyRecords, structuredEvents]);

  const filteredTimeline = useMemo(() => timeline.filter((event) => (
    (sourceFilter === "all" || event.source === sourceFilter)
    && (severityFilter === "all" || event.severity === severityFilter)
  )), [severityFilter, sourceFilter, timeline]);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(copyPlaintext(filteredTimeline));
      showSuccess("Safe diagnostic text copied to clipboard.");
    } catch {
      showError("Could not copy diagnostic text.");
    }
  }, [filteredTimeline]);

  if (unavailable) {
    return (
      <div className="text-[12px] leading-5 text-[var(--text-dim)]">
        Diagnostic Center is unavailable in this installed build. Update WinCommander to view diagnostics.
        <div className="mt-3"><Button text="Refresh" icon="refresh" className="compact-action-btn" loading={loading} onClick={refresh} /></div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="m-0 max-w-2xl text-[12px] leading-5 text-[var(--text-dim)]">
          One safe diagnostic history for the retained seven-day window. Legacy free-text details are intentionally hidden because they have no privacy classification.
        </p>
        <div className="flex gap-2">
          <Button text="Refresh" icon="refresh" className="compact-action-btn" loading={loading} onClick={refresh} />
          <Button text="Copy safe text" icon="duplicate" className="compact-action-btn" disabled={filteredTimeline.length === 0} onClick={handleCopy} />
        </div>
      </div>

      {health && (
        <div className="grid grid-cols-2 gap-2 md:grid-cols-4" aria-label="Diagnostics health">
          <Metric label="Store" value={health.healthy ? "Healthy" : "Needs attention"} />
          <Metric label="Stored" value={String(health.persistedEvents)} />
          <Metric label="Dropped" value={String(health.droppedEvents)} />
          <Metric label="Redacted" value={String(health.redactedFields)} />
        </div>
      )}

      <div className="flex flex-wrap gap-3" aria-label="Diagnostic filters">
        <FilterGroup label="Source" options={SOURCE_FILTERS} selected={sourceFilter} onSelect={setSourceFilter} />
        <FilterGroup label="Severity" options={SEVERITY_FILTERS} selected={severityFilter} onSelect={setSeverityFilter} />
      </div>

      {!loading && filteredTimeline.length === 0 && (
        <p className="m-0 text-[12px] text-[var(--text-mute)]">No diagnostic records match these filters.</p>
      )}

      <div className="flex max-h-[520px] min-h-[220px] flex-col gap-2 overflow-y-auto pr-1" aria-live="polite" aria-label="Unified diagnostic timeline">
        {filteredTimeline.map((event) => (
          <article key={event.id} className="rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3">
            <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
              <span className="font-[family-name:var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--text-mute)]">
                {event.sourceDetail ?? event.source} · {event.severity}
              </span>
              <span className="text-[10px] text-[var(--text-mute)]">{timestamp(event.occurredAt)}</span>
            </div>
            <div className="mt-1 text-[13px] font-medium text-[var(--text)]">{event.summary}</div>
            {event.operationId && <div className="mt-1 font-[family-name:var(--font-mono)] text-[10px] text-[var(--text-mute)]">Operation: {event.operationId}</div>}
            {(event.detail || event.errorCode || event.nextAction) && (
              <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-[var(--text-dim)]">
                {event.detail && <span>{event.detail}</span>}
                {event.errorCode && <span>Code: {event.errorCode}</span>}
                {event.nextAction && <span>Next: {event.nextAction}</span>}
              </div>
            )}
          </article>
        ))}
      </div>
    </div>
  );
}

function FilterGroup<T extends string>({
  label,
  options,
  selected,
  onSelect,
}: {
  label: string;
  options: Array<{ value: T; label: string }>;
  selected: T;
  onSelect: (value: T) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label={`${label} filters`}>
      <span className="text-[10px] uppercase tracking-wider text-[var(--text-mute)]">{label}</span>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={`rounded-[var(--r-sm)] border px-2 py-1 text-[10px] ${selected === option.value ? "border-[var(--accent)] bg-[var(--surface-3)] text-[var(--text)]" : "border-[var(--border)] bg-[var(--surface-2)] text-[var(--text-mute)]"}`}
          aria-pressed={selected === option.value}
          onClick={() => onSelect(option.value)}
        >
          {option.label}
        </button>
      ))}
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
