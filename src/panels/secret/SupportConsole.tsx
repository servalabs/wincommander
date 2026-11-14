import { useCallback, useMemo, useState } from "react";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";
import { safeLegacyDiagnosticSource, sanitizeLegacyDiagnosticMessage } from "../../lib/diagnosticSanitizer";
import { useDiagnosticCenter, type DiagnosticEvent } from "../../hooks/useDiagnosticCenter";
import { showError, showSuccess } from "../../utils/toast";
import { Copy, Download, RefreshCw } from "lucide-react";
import { useMaintenanceHistory } from "../../hooks/useMaintenanceHistory";
import { maintenanceLogEvent } from "./maintenanceLogView";

type LegacyRecord = { date: string; timestamp: string; level: string; source: string; message: string };

type SourceFilter = "all" | "desktop" | "service" | "pro" | "legacy" | "maintenance";
type SeverityFilter = "all" | "error" | "warn" | "info";

type TimelineEvent = {
  id: string;
  occurredAt: string;
  source: "desktop" | "service" | "pro" | "legacy" | "maintenance";
  sourceDetail?: string;
  severity: "error" | "warn" | "info";
  summary: string;
  operationId?: string;
  detail?: string;
  errorCode?: string;
  nextAction?: string;
  safeSummary?: string;
  safeDetail?: string;
};

const SOURCE_FILTERS: Array<{ value: SourceFilter; label: string }> = [
  { value: "all", label: "All sources" },
  { value: "desktop", label: "Desktop" },
  { value: "service", label: "Service" },
  { value: "pro", label: "Pro" },
  { value: "legacy", label: "Legacy" },
  { value: "maintenance", label: "Maintenance" },
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
    event.safeSummary ?? event.summary,
    event.operationId ? `operation=${event.operationId}` : "",
    event.errorCode ? `code=${event.errorCode}` : "",
    event.source === "maintenance" ? event.safeDetail ?? "" : event.detail ?? "",
    event.nextAction ? `next=${event.nextAction}` : "",
  ].filter(Boolean).join(" ")).join("\n");
}

export default function SupportConsole() {
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>("all");
  const [severityFilter, setSeverityFilter] = useState<SeverityFilter>("all");
  const { structuredEvents, legacyRecords, health, loading, unavailable, refresh } = useDiagnosticCenter();
  const maintenanceHistory = useMaintenanceHistory();
  const requestConfirm = useAppConfirm();

  const timeline = useMemo<TimelineEvent[]>(() => [
    ...structuredEvents.map((event) => structuredTimelineEvent(event, event.source ?? "desktop")),
    ...legacyRecords.map(legacyTimelineEvent),
    ...maintenanceHistory.operations.map(maintenanceLogEvent),
  ].sort((left, right) => right.occurredAt.localeCompare(left.occurredAt)), [legacyRecords, maintenanceHistory.operations, structuredEvents]);

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

  const handleSave = useCallback(() => {
    const url = URL.createObjectURL(new Blob([copyPlaintext(filteredTimeline)], { type: "text/plain;charset=utf-8" }));
    const link = document.createElement("a");
    link.href = url;
    link.download = "wincommander-logs.txt";
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }, [filteredTimeline]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="m-0 max-w-2xl text-[12px] leading-5 text-[var(--text-dim)]">
          Diagnostic logs cover the retained seven-day window. Maintenance keeps the latest 100 local results separately. Legacy free-text details are intentionally hidden because they have no privacy classification. Maintenance details stay on this device and are omitted from safe text exports.
        </p>
        <div className="flex gap-2">
          <button type="button" className="compact-action-btn" title="Refresh logs" aria-label="Refresh logs" disabled={loading} onClick={refresh}><RefreshCw size={15} /></button>
          <button type="button" className="compact-action-btn" title="Copy safe text" aria-label="Copy safe text" disabled={filteredTimeline.length === 0} onClick={handleCopy}><Copy size={15} /></button>
          <button type="button" className="compact-action-btn" title="Save logs" aria-label="Save logs" disabled={filteredTimeline.length === 0} onClick={handleSave}><Download size={15} /></button>
        </div>
      </div>

      {unavailable && <p role="status" className="text-xs text-[var(--warn)]">Diagnostic logs are unavailable in this build; local maintenance results remain available.</p>}
      {!maintenanceHistory.isPersistent && <p role="status" className="text-xs text-[var(--warn)]">Local maintenance history is unavailable; new results remain for this session only.</p>}
      {health && !health.healthy && <p role="status" className="text-xs text-[var(--warn)]">Log storage needs attention; some events may be unavailable.</p>}
        <div className="grid grid-cols-2 gap-2 md:grid-cols-5" aria-label="Loaded log counts">
          <Metric label="Warnings" value={String(timeline.filter((event) => event.severity === "warn").length)} />
          <Metric label="Errors" value={String(timeline.filter((event) => event.severity === "error").length)} />
          <Metric label="Desktop" value={String(timeline.filter((event) => event.source === "desktop").length)} />
          <Metric label="Service" value={String(timeline.filter((event) => event.source === "service").length)} />
          <Metric label="Pro" value={String(timeline.filter((event) => event.source === "pro").length)} />
        </div>

      <div className="flex flex-wrap gap-3" aria-label="Diagnostic filters">
        <FilterGroup label="Source" options={SOURCE_FILTERS} selected={sourceFilter} onSelect={setSourceFilter} />
        <FilterGroup label="Severity" options={SEVERITY_FILTERS} selected={severityFilter} onSelect={setSeverityFilter} />
        {maintenanceHistory.operations.length > 0 && <button type="button" className="compact-action-btn ml-auto" onClick={() => void (async () => {
          if (await requestConfirm({ title: "Clear local maintenance history?", description: "This removes local display records; it does not undo operations or clear diagnostic logs.", confirmLabel: "Clear history" })) maintenanceHistory.clearHistory();
        })()}>Clear maintenance history</button>}
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
