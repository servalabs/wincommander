import { useCallback, useMemo } from "react";
import SectionCard from "../shared/SectionCard";
import UniversalToggle from "../shared/UniversalToggle";
import useBackend from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import { runOperation } from "../../context/OperationContext";
import { showSuccess } from "../../utils/toast";
import { formatMaintenanceSuccess } from "../../utils/maintenance";

// "Optimize Services" lives in the Service Manager subpanel as
// "Apply Recommended Tweaks" so the one-shot bulk action is in the
// same place as the granular per-service controls. Don't add a second
// button here.

/**
 * Lump of four maintenance action buttons that all follow the same
 * "show status bar → call backend → record runCount + lastRunAt in
 * settings → toast" lifecycle. Lived inline in TweaksPanel for a while;
 * moved out so the panel can stay close to a pure renderer.
 */
export default function SystemMaintenanceCard({
    isAdvanced,
    localLoadingMap,
    setLocalLoadingMap,
}: {
    isAdvanced: boolean;
    localLoadingMap: Record<string, boolean>;
    setLocalLoadingMap: React.Dispatch<React.SetStateAction<Record<string, boolean>>>;
}) {
    const { appSettings, patchAppSettings } = useAppState();
    const { invokeSystemRepair, invokeWindowsUpdateRepair, invokeDefrag } = useBackend();

    const maintenanceRuns = useMemo(
        () => appSettings?.ideal?.tweaks?.maintenanceRuns ?? {},
        [appSettings?.ideal?.tweaks?.maintenanceRuns],
    );

    const getInfo = useCallback((key: string) => {
        const info = maintenanceRuns[key];
        const runCount = info?.runCount ?? 0;
        const lastRunAt = info?.lastRunAt ?? null;
        const lastRun = lastRunAt ? new Date(lastRunAt) : null;
        const validLastRun = lastRun && !Number.isNaN(lastRun.getTime()) ? lastRun : null;
        const ageDays = validLastRun ? Math.floor((Date.now() - validLastRun.getTime()) / 86_400_000) : null;
        const fresh = ageDays !== null && ageDays <= 15;
        const lastLabel = !validLastRun
            ? "Never run"
            : ageDays === 0 ? "Run today"
            : ageDays === 1 ? "Run yesterday"
            : `Run ${ageDays} days ago`;
        return { runCount, ageDays, fresh, lastLabel };
    }, [maintenanceRuns]);

    const getDescription = useCallback((key: string, base: string) => {
        const info = getInfo(key);
        return `${base} • ${info.lastLabel} • ${info.runCount} run${info.runCount === 1 ? "" : "s"}`;
    }, [getInfo]);

    const getClass = useCallback((key: string) => {
        const info = getInfo(key);
        if (info.runCount === 0) return "maintenance-never";
        return info.fresh ? "maintenance-fresh" : "maintenance-stale";
    }, [getInfo]);

    const run = useCallback(async (key: string, label: string, fn: () => Promise<any>) => {
        setLocalLoadingMap(prev => ({ ...prev, [key]: true }));
        let captured: any = null;
        const wrapped = async () => { const r = await fn(); captured = r; return r; };
        try {
            await runOperation(label, [{ label: `Running ${label.toLowerCase()}...`, fn: wrapped }],
                { mode: 'sequential', accent: 'neutral' });
            const previous = appSettings?.ideal?.tweaks?.maintenanceRuns?.[key];
            await patchAppSettings({
                ideal: { tweaks: { maintenanceRuns: {
                    [key]: { lastRunAt: new Date().toISOString(), runCount: (previous?.runCount ?? 0) + 1 },
                } } },
            });
            showSuccess(formatMaintenanceSuccess(label, captured));
        } catch {
            // runOperation already surfaces the error in the status bar.
        } finally {
            setLocalLoadingMap(prev => ({ ...prev, [key]: false }));
        }
    }, [appSettings?.ideal?.tweaks?.maintenanceRuns, patchAppSettings, setLocalLoadingMap]);

    // Note: "Optimize Services" moved to Service Manager's "Apply
    // Recommended Tweaks" button. "Disk Cleanup" moved to Granular Disk
    // Cleanup's "Clean All" button — keeps the one-shot bulk-clean
    // next to the per-category granular controls in one panel.
    const actions: Array<{ key: string; label: string; advLabel: string; desc: string; icon: any; sev: any; fn: () => Promise<any> }> = [
        { key: "repair",  label: "Deep Fix",         advLabel: "System Repair",  desc: "Run SFC and DISM Fix",            icon: "build",                sev: "primary", fn: invokeSystemRepair },
        {
            key: "updateRepair",
            label: "Fix Updates",
            advLabel: "Windows Update Repair",
            desc: "Reset update caches, services, network state, DISM, and SFC",
            icon: "automatic-updates",
            sev: "primary",
            fn: invokeWindowsUpdateRepair,
        },
        { key: "defrag",  label: "Optimize Storage", advLabel: "Defrag / Trim",  desc: "Auto-defrag on HDDs · TRIM on SSDs", icon: "predictive-analysis", sev: "none",    fn: invokeDefrag },
    ];

    return (
        <SectionCard title={isAdvanced ? "System Maintenance" : "Cleanup & Fixes"} icon="wrench">
            <div className="flex flex-col gap-2">
                {actions.map(a => (
                    <UniversalToggle
                        key={a.key}
                        label={isAdvanced ? a.advLabel : a.label}
                        description={getDescription(a.key, a.desc)}
                        checked={Boolean(localLoadingMap[a.key]) || getInfo(a.key).fresh}
                        onChange={() => run(a.key, `${a.advLabel === "Defrag / Trim" ? "Defrag / Trim Drive" : (a.advLabel === "System Repair" ? "System Repair (SFC + DISM)" : a.advLabel)}`, a.fn)}
                        isAction
                        icon={a.icon}
                        severity={a.sev}
                        loading={localLoadingMap[a.key]}
                        className={`maintenance-action ${localLoadingMap[a.key] ? "maintenance-running" : getClass(a.key)}`}
                    />
                ))}
            </div>
        </SectionCard>
    );
}
