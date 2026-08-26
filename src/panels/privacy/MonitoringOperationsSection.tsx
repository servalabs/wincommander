import { useMemo, useState } from "react";
import { Button, Icon, Spinner } from "@/components/ui/bp";
import SectionCard from "@/components/shared/SectionCard";
import {
  type MonitorSnapshot,
  type MonitorState,
  type MonitoringOverviewState,
} from "@/hooks/useMonitoringOverview";

const STATE_ORDER: MonitorState[] = ["alert", "degraded", "stale", "unavailable", "active", "idle", "locked"];

function stateLabel(state: MonitorState): string {
  return state === "stale" ? "STALE" : state.toUpperCase();
}

function stateColor(state: MonitorState): string {
  switch (state) {
    case "alert":
      return "var(--color-danger,#f87171)";
    case "degraded":
    case "stale":
      return "var(--color-warning,#fbbf24)";
    case "active":
      return "var(--color-success,#4ade80)";
    case "locked":
      return "var(--color-accent,#60a5fa)";
    default:
      return "var(--color-text-muted,#94a3b8)";
  }
}

function relativeTime(iso: string | null): string {
  if (!iso) return "Not observed";
  const parsed = Date.parse(iso);
  if (Number.isNaN(parsed)) return "Unknown";
  const seconds = Math.max(0, Math.round((Date.now() - parsed) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

function count(value: number | null): string {
  return value === null ? "—" : value.toLocaleString();
}

function errorLabel(code: string | null): string | null {
  switch (code) {
    case "entitlement_required":
      return "Pro entitlement required";
    case "pro_unavailable":
      return "Pro monitor service unavailable";
    case "stale_poll":
      return "No recent collector check-in";
    case "configuration_conflict":
      return "Configuration needs review";
    case "attribution_unavailable":
      return "Behavior detection is running; process attribution is unavailable";
    case "listener_or_policy_degraded":
      return "Clipboard listener or policy health needs review";
    case "channel_missing":
      return "Windows print channel is not available";
    case "collector_error":
      return "Collector reported an error";
    case "policy_not_compiled":
      return "Monitor policy is not ready";
    case "evidence_loss":
      return "Some queued monitor reports were lost";
    case "agent_unavailable":
      return "Authoritative monitor service is unavailable";
    case "snapshot_timeout":
      return "Monitor check timed out";
    default:
      return code ? "Status could not be verified" : null;
  }
}

function MonitorRow({ monitor, showCapabilities }: { monitor: MonitorSnapshot; showCapabilities: boolean }) {
  const color = stateColor(monitor.state);
  const lastSeen = monitor.lastActivityAt ?? monitor.startedAt;
  return (
    <div
      className="rounded-lg border p-3"
      style={{ borderColor: `color-mix(in srgb, ${color} 30%, var(--color-border))` }}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-sm font-semibold truncate">{monitor.label}</div>
          <div className="mt-1 text-[10px] uppercase tracking-wider text-[var(--color-text-muted)]">
            {monitor.group} · {monitor.requiresPro ? "Pro" : "Free"}
          </div>
        </div>
        <span
          className="flex-shrink-0 rounded border px-2 py-0.5 font-mono text-[10px]"
          style={{ color, borderColor: `color-mix(in srgb, ${color} 45%, transparent)`, background: `color-mix(in srgb, ${color} 10%, transparent)` }}
        >
          {stateLabel(monitor.state)}
        </span>
      </div>

      <div className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-xs text-[var(--color-text-muted)] sm:grid-cols-4">
        <span>Recent <strong className="ml-1 text-[var(--color-text)]">{count(monitor.recentCount)}</strong></span>
        <span>Active <strong className="ml-1 text-[var(--color-text)]">{count(monitor.activeCount)}</strong></span>
        <span>Last check <strong className="ml-1 text-[var(--color-text)]">{relativeTime(lastSeen)}</strong></span>
        <span>Cadence <strong className="ml-1 text-[var(--color-text)]">{monitor.cadenceSecs ? `${monitor.cadenceSecs}s` : "event"}</strong></span>
      </div>

      {showCapabilities && monitor.capabilities.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-1.5">
          {monitor.capabilities.map((capability) => (
            <span key={capability} className="rounded border border-[var(--color-border)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-muted)]">
              {capability.replace(/-/g, " ")}
            </span>
          ))}
        </div>
      )}

      {monitor.errorCode && (
        <div className="mt-3 flex items-center gap-1.5 text-[11px]" style={{ color }}>
          <Icon icon="warning-sign" size={12} />
          <span>{errorLabel(monitor.errorCode)}</span>
        </div>
      )}
    </div>
  );
}

export default function MonitoringOperationsSection({
  overview,
  error,
  refresh,
  loading,
  refreshing,
}: MonitoringOverviewState) {
  const [group, setGroup] = useState("all");
  const [state, setState] = useState<MonitorState | "all">("all");
  const [attentionOnly, setAttentionOnly] = useState(false);
  const [showCapabilities, setShowCapabilities] = useState(false);

  const groups = useMemo(
    () => [...new Set(overview?.monitors.map((monitor) => monitor.group) ?? [])].sort(),
    [overview],
  );
  const monitors = useMemo(() => {
    if (!overview) return [];
    return overview.monitors
      .filter((monitor) => group === "all" || monitor.group === group)
      .filter((monitor) => state === "all" || monitor.state === state)
      .filter((monitor) => !attentionOnly || ["alert", "degraded", "stale", "unavailable"].includes(monitor.state))
      .sort((a, b) => STATE_ORDER.indexOf(a.state) - STATE_ORDER.indexOf(b.state) || a.label.localeCompare(b.label));
  }, [attentionOnly, group, overview, state]);

  const summary = overview?.summary;
  const attentionCount = (summary?.alerts ?? 0) + (summary?.degraded ?? 0) + (summary?.unavailable ?? 0);

  return (
    <SectionCard
      title="Monitor Operations Center"
      icon="pulse"
      headerRight={loading ? <Spinner size={14} /> : <span className="font-mono text-[10px] text-[var(--color-text-muted)]">{summary?.active ?? 0} ACTIVE</span>}
    >
      <div className="flex flex-col gap-4">
        <div className="flex items-start gap-3">
          <Icon icon="dashboard" size={18} color="var(--color-accent)" className="mt-0.5 flex-shrink-0" />
          <p className="max-w-3xl text-xs leading-relaxed text-[var(--color-text-muted)]">
            Think of this as the control tower: one quick view of which monitors are on,
            which need attention, and which are only available with Pro. Detailed cards
            below still contain each monitor&apos;s controls and event history.
          </p>
        </div>

        {summary && (
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
            {[
              ["Active", summary.active, "var(--color-success)"],
              ["Alerts", summary.alerts, "var(--color-danger,#f87171)"],
              ["Degraded", summary.degraded, "var(--color-warning,#fbbf24)"],
              ["Unavailable", summary.unavailable, "var(--color-text-muted)"],
              ["Pro locked", summary.locked, "var(--color-accent,#60a5fa)"],
            ].map(([label, value, color]) => (
              <div key={label as string} className="rounded-md border border-[var(--color-border)] px-3 py-2">
                <div className="font-mono text-lg font-semibold" style={{ color: color as string }}>{value as number}</div>
                <div className="text-[10px] uppercase tracking-wider text-[var(--color-text-muted)]">{label as string}</div>
              </div>
            ))}
          </div>
        )}

        <div className="flex flex-wrap items-end gap-3 border-y border-[var(--color-border)] py-3">
          <label className="flex flex-col gap-1 text-[10px] uppercase tracking-wider text-[var(--color-text-muted)]">
            Group
            <select value={group} onChange={(event) => setGroup(event.currentTarget.value)} className="rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-2 py-1.5 text-xs normal-case tracking-normal text-[var(--color-text)]">
              <option value="all">All groups</option>
              {groups.map((item) => <option key={item} value={item}>{item}</option>)}
            </select>
          </label>
          <label className="flex flex-col gap-1 text-[10px] uppercase tracking-wider text-[var(--color-text-muted)]">
            State
            <select value={state} onChange={(event) => setState(event.currentTarget.value as MonitorState | "all")} className="rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-2 py-1.5 text-xs normal-case tracking-normal text-[var(--color-text)]">
              <option value="all">All states</option>
              {STATE_ORDER.map((item) => <option key={item} value={item}>{stateLabel(item)}</option>)}
            </select>
          </label>
          <label className="flex items-center gap-2 pb-1.5 text-xs text-[var(--color-text-muted)]">
            <input type="checkbox" checked={attentionOnly} onChange={(event) => setAttentionOnly(event.currentTarget.checked)} />
            Needs attention ({attentionCount})
          </label>
          <label className="flex items-center gap-2 pb-1.5 text-xs text-[var(--color-text-muted)]">
            <input type="checkbox" checked={showCapabilities} onChange={(event) => setShowCapabilities(event.currentTarget.checked)} />
            Show coverage
          </label>
          <Button icon="refresh" minimal small disabled={refreshing} onClick={() => void refresh()} className="ml-auto">
            Refresh all
          </Button>
        </div>

        {error && <div role="alert" className="text-xs text-[var(--color-danger,#f87171)]">{error}</div>}
        {overview && <div className="text-[10px] text-[var(--color-text-muted)]">Checked {relativeTime(overview.observedAt)} · {monitors.length} shown · {overview.privacy.contentFree ? "content-free summary" : "review privacy scope"}</div>}
        {loading && <div className="flex items-center gap-2 text-xs text-[var(--color-text-muted)]"><Spinner size={14} /> Checking monitors…</div>}
        {overview && monitors.length === 0 && <div className="text-xs text-[var(--color-text-muted)]">No monitors match these filters.</div>}
        {overview && monitors.length > 0 && <div className="grid gap-2 lg:grid-cols-2">{monitors.map((monitor) => <MonitorRow key={monitor.id} monitor={monitor} showCapabilities={showCapabilities} />)}</div>}
      </div>
    </SectionCard>
  );
}
