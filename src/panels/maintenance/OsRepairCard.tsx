import { useCallback, useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import useBackend from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import { runOperation } from "../../context/OperationContext";
import { showSuccess } from "../../utils/toast";
import { formatMaintenanceSuccess } from "../../utils/maintenance";

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
  { key: "repair", label: "System repair", operationLabel: "System Repair (SFC + DISM)", description: "Verify and restore Windows system files with SFC, then repair the component store with DISM.", icon: "build" },
  { key: "updateRepair", label: "Windows Update repair", operationLabel: "Windows Update Repair", description: "Reset update caches, services, Winsock, and proxy state, then re-run DISM and SFC.", icon: "automatic-updates" },
  { key: "defrag", label: "Defrag / TRIM", operationLabel: "Defrag / Trim Drive", description: "Defragment mechanical drives; issue TRIM on solid-state drives. Windows picks the right operation per disk.", icon: "predictive-analysis" },
] as const;

// A repair older than this reads as stale. Matches the ageing threshold the
// Windows Settings card used so existing history keeps its meaning.
const FRESH_DAYS = 15;

export default function OsRepairCard() {
  const { appSettings, patchAppSettings } = useAppState();
  const { invokeSystemRepair, invokeWindowsUpdateRepair, invokeDefrag } = useBackend();
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
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Windows repair</CardTitle>
        <CardDescription>Microsoft's own repair tooling. Each run is long, needs Administrator, and reports its result in the status bar. Nothing here removes your files.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {REPAIR_ACTIONS.map((action) => {
          const history = getHistory(action.key);
          const isBusy = !!busy[action.key];
          return (
            <div key={action.key} className="flex flex-wrap items-start gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3">
              <Icon icon={action.icon} className="mt-0.5 shrink-0 text-[var(--accent)]" />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-sm font-medium text-[var(--text)]">{action.label}</span>
                  <Badge tone={history.tone}>{history.label}</Badge>
                  {history.runCount > 0 && <span className="font-mono text-[10.5px] text-[var(--text-mute)]">{history.runCount} run{history.runCount === 1 ? "" : "s"}</span>}
                </div>
                <p className="mt-1 text-xs text-[var(--text-dim)]">{action.description}</p>
              </div>
              <Button size="sm" variant="outline" disabled={isBusy} onClick={() => void run(action.key, action.operationLabel, handlers[action.key])}>
                <Icon icon={isBusy ? "refresh" : "play"} className={isBusy ? "animate-spin" : undefined} />{isBusy ? "Running…" : "Run"}
              </Button>
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}
