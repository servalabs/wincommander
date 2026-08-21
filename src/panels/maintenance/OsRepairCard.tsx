import { useCallback, useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import useBackend from "../../hooks/useBackend";
import TierGate from "../../components/shared/TierGate";
import { useAppState } from "../../context/AppContext";
import { runOperation } from "../../context/OperationContext";
import { showSuccess } from "../../utils/toast";
import { formatMaintenanceSuccess } from "../../utils/maintenance";
import RunOnceButton from "../../components/cleanup/RunOnceButton";

/**
 * Windows' own repair tooling — SFC/DISM, the Windows Update reset chain, and
 * defrag/TRIM. Lived in Windows Settings as "System Maintenance"/"Deep Fix",
 * which collided with Maintenance's own "System repair" tab (broken shortcuts,
 * PATH, uninstall leftovers). Two unrelated things called the same name; the
 * OS-repair half belongs here, where users look for it.
 *
 * `maintenanceRuns` keys are load-bearing: they carry the existing run history
 * users already accumulated under Windows Settings.
 */
const REPAIR_ACTIONS = [
  { key: "repair", label: "System repair", operationLabel: "System Repair (SFC + DISM)", description: "Verify and restore Windows system files with SFC, then repair the component store with DISM.", icon: "build", tier: "free" },
  { key: "updateRepair", label: "Windows Update repair", operationLabel: "Windows Update Repair", description: "Reset update caches, services, Winsock, and proxy state, then re-run DISM and SFC.", icon: "automatic-updates", tier: "free" },
  { key: "defrag", label: "Defrag / TRIM", operationLabel: "Defrag / Trim Drive", description: "Defragment mechanical drives; issue TRIM on solid-state drives. Windows picks the right operation per disk.", icon: "predictive-analysis", tier: "free" },
  // Moved here from System Cleanup per product decision (2026-07): this is the
  // paid, Pro-sidecar forced retrim, distinct from the free per-volume
  // Optimize-Volume pass above -- kept alongside it so both TRIM actions live
  // in one place.
  { key: "ssdTrim", label: "Force SSD TRIM", operationLabel: "Force SSD TRIM", description: "Force an immediate TRIM pass on every SSD through the Pro sidecar, bypassing Windows' own schedule.", icon: "flash", tier: "paid" },
] as const;

// A repair older than this reads as stale. Matches the ageing threshold the
// Windows Settings card used so existing history keeps its meaning.
const FRESH_DAYS = 15;

interface OsRepairCardProps {
  embedded?: boolean;
}

export default function OsRepairCard({ embedded = false }: OsRepairCardProps) {
  const { appSettings, patchAppSettings } = useAppState();
  const { invokeSystemRepair, invokeWindowsUpdateRepair, invokeDefrag, invokeSSDTrim } = useBackend();
  const [busy, setBusy] = useState<Record<string, boolean>>({});

  const maintenanceRuns = useMemo(
    () => appSettings?.ideal?.tweaks?.maintenanceRuns ?? {},
    [appSettings?.ideal?.tweaks?.maintenanceRuns],
  );

  const getHistory = useCallback((key: string) => {
    const info = maintenanceRuns[key];
    const runCount = info?.runCount ?? 0;
    const parsed = info?.lastRunAt ? new Date(info.lastRunAt) : null;
    const lastRun = parsed && !Number.isNaN(parsed.getTime()) ? parsed : null;
    const ageDays = lastRun ? Math.floor((Date.now() - lastRun.getTime()) / 86_400_000) : null;
    const label = ageDays === null ? "Never run"
      : ageDays === 0 ? "Run today"
      : ageDays === 1 ? "Run yesterday"
      : `Run ${ageDays} days ago`;
    const tone = ageDays === null ? "neutral" : ageDays <= FRESH_DAYS ? "success" : "warning";
    return { runCount, label, tone } as const;
  }, [maintenanceRuns]);

  const run = useCallback(async (key: string, operationLabel: string, fn: () => Promise<unknown>) => {
    setBusy(prev => ({ ...prev, [key]: true }));
    let captured: unknown = null;
    try {
      await runOperation(
        operationLabel,
        [{ label: `Running ${operationLabel.toLowerCase()}...`, fn: async () => { const result = await fn(); captured = result; return result; } }],
        { mode: "sequential", accent: "neutral" },
      );
      const previous = appSettings?.ideal?.tweaks?.maintenanceRuns?.[key];
      await patchAppSettings({
        ideal: { tweaks: { maintenanceRuns: {
          [key]: { lastRunAt: new Date().toISOString(), runCount: (previous?.runCount ?? 0) + 1 },
        } } },
      });
      showSuccess(formatMaintenanceSuccess(operationLabel, captured));
    } catch {
      // runOperation already surfaces the error in the status bar.
    } finally {
      setBusy(prev => ({ ...prev, [key]: false }));
    }
  }, [appSettings?.ideal?.tweaks?.maintenanceRuns, patchAppSettings]);

  const handlers: Record<string, () => Promise<unknown>> = {
    repair: invokeSystemRepair,
    updateRepair: invokeWindowsUpdateRepair,
    defrag: invokeDefrag,
    ssdTrim: invokeSSDTrim,
  };

  const actions = (
    <>
      {!embedded && <CardHeader>
        <CardTitle>Windows repair</CardTitle>
        <CardDescription>Microsoft's own repair tooling. Each run is long, needs Administrator, and reports its result in the status bar. Nothing here removes your files.</CardDescription>
      </CardHeader>}
      <CardContent className={embedded ? "grid grid-cols-1 gap-2 p-0 md:grid-cols-2 xl:grid-cols-3" : "flex flex-col gap-2"}>
        {REPAIR_ACTIONS.map((action) => {
          const history = getHistory(action.key);
          const isBusy = !!busy[action.key];
          const runButton = (
            embedded ? (
              <RunOnceButton
                isRunning={isBusy}
                onClick={() => void run(action.key, action.operationLabel, handlers[action.key])}
                className="shrink-0"
                actionLabel={action.label}
              />
            ) : (
              <Button
                size="sm"
                variant="outline"
                aria-label={isBusy ? `${action.label} is running` : `Run ${action.label}`}
                disabled={isBusy}
                onClick={() => void run(action.key, action.operationLabel, handlers[action.key])}
              >
                <Icon icon={isBusy ? "refresh" : "play"} className={isBusy ? "animate-spin" : undefined} />{isBusy ? "Running…" : "Run"}
              </Button>
            )
          );
          return (
            <div key={action.key} className={embedded
              ? "flex min-h-[104px] items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-bg-card)] px-3 py-2.5"
              : "flex flex-wrap items-start gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3"}
            >
              {embedded ? (
                <div className="flex size-[30px] shrink-0 items-center justify-center rounded-md bg-[var(--color-accent)]/10">
                  <Icon icon={action.icon} size={15} className="text-[var(--color-accent)]" />
                </div>
              ) : <Icon icon={action.icon} className="mt-0.5 shrink-0 text-[var(--accent)]" />}
              <div className="min-w-0 flex-1">
                <div className={embedded ? "flex min-w-0 items-center gap-2" : "flex flex-wrap items-center gap-2"}>
                  <span className={embedded ? "truncate text-[11px] font-bold uppercase tracking-[0.4px] text-[var(--color-text-primary)]" : "text-sm font-medium text-[var(--text)]"}>{action.label}</span>
                  {!embedded && <Badge tone={history.tone}>{history.label}</Badge>}
                  {!embedded && history.runCount > 0 && <span className="font-mono text-[10.5px] text-[var(--text-mute)]">{history.runCount} run{history.runCount === 1 ? "" : "s"}</span>}
                </div>
                <p className={embedded ? "truncate text-[10.5px] text-[var(--color-text-muted)]" : "mt-1 text-xs text-[var(--text-dim)]"}>{action.description}</p>
                {embedded && <span className="block truncate text-[9px] text-[var(--color-text-muted)] opacity-70">{history.label}{history.runCount > 0 ? ` · ${history.runCount} run${history.runCount === 1 ? "" : "s"}` : ""}</span>}
              </div>
              {action.tier === "paid" ? <TierGate tier="paid" featureLabel={action.label}>{runButton}</TierGate> : runButton}
            </div>
          );
        })}
      </CardContent>
    </>
  );

  return embedded ? actions : <Card>{actions}</Card>;
}
