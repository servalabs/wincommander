import { useCallback, useEffect, useState } from "react";
import { Badge } from "../../ui/badge";
import { Button } from "../../ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuLabel, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "../../ui/dropdown-menu";
import { Icon } from "../../ui/icon";
import { Spinner } from "../../ui/spinner";
import useBackend, { executeBackendCommand } from "../../../hooks/useBackend";
import useEntitlements from "../../../hooks/useEntitlements";
import { useAppState } from "../../../context/AppContext";
import { runOperation } from "../../../context/OperationContext";
import { showSuccess, showError } from "../../../utils/toast";
import { formatMaintenanceSuccess } from "../../../utils/maintenance";

interface CleanupCategory {
    Id: string;
    Label: string;
    Path: string;
    Exists: boolean;
    FileCount: number;
    SizeBytes: number;
    SizeMb: number;
}

// Auto-clean presets. 60 minutes is the floor the scheduler enforces — a
// cleanmgr sweep is too heavy to run more often than hourly.
const SCHEDULE_PRESETS = [
    { minutes: 60, label: "Every hour" },
    { minutes: 360, label: "Every 6 hours" },
    { minutes: 720, label: "Every 12 hours" },
    { minutes: 1440, label: "Daily" },
    { minutes: 10080, label: "Weekly" },
];
const SCHEDULE_OFF = "off";

function formatInterval(minutes: number): string {
    if (minutes < 60) return `${minutes}m`;
    if (minutes < 1440) return `${minutes / 60}h`;
    if (minutes < 10080) return `${minutes / 1440}d`;
    return `${minutes / 10080}w`;
}

// ── Module-level cache ───────────────────────────────────────────────
// Persists the scan result across DiskCleanupGranular mount/unmount so
// navigating away and back shows the "MB selected" counter instantly, and
// lets an app-startup pre-load (see preloadDiskCleanupScan, wired from
// App.tsx right after splash) populate it before the user ever opens
// Maintenance — no loading flash on first visit. Mirrors the
// cache/subscriber convention used by src/panels/cleanup/useCleanupScan.ts
// (_cleanupCache / updateCacheEntry).
let _diskCleanupCache: CleanupCategory[] | null = null;
let _diskCleanupInFlight: Promise<DiskCleanupScanResult> | null = null;
let _diskCleanupSubscriber: ((cats: CleanupCategory[]) => void) | null = null;

export type DiskCleanupScanResult =
    | { success: true; categories: CleanupCategory[] }
    | { success: false; error?: string };

// Concurrent callers share one in-flight PowerShell scan — Get-DiskCleanupScan
// walks Temp/Windows Update cache/Prefetch/Windows.old and can take seconds, so
// parallel scans are expensive. Writes the module cache and notifies whichever
// instance (if any) is currently mounted/subscribed.
export function fetchDiskCleanupScan(): Promise<DiskCleanupScanResult> {
    if (_diskCleanupInFlight) return _diskCleanupInFlight;
    const run = async (): Promise<DiskCleanupScanResult> => {
        const res = await executeBackendCommand<{ categories: CleanupCategory[] }>("Get-DiskCleanupScan");
        if (res.success && res.data?.categories) {
            _diskCleanupCache = res.data.categories;
            _diskCleanupSubscriber?.(res.data.categories);
            return { success: true, categories: res.data.categories };
        }
        return { success: false, error: res.error };
    };
    const p = run();
    _diskCleanupInFlight = p;
    p.finally(() => { if (_diskCleanupInFlight === p) _diskCleanupInFlight = null; });
    return p;
}

// Fire-and-forget pre-load called once from App.tsx right after splash, so the
// scan is in flight before the user reaches Maintenance. Failures are swallowed:
// the panel just scans normally on first open.
export function preloadDiskCleanupScan(): void {
    if (_diskCleanupCache) return; // already populated by an earlier call
    fetchDiskCleanupScan().catch(() => {});
}

const defaultSelection = (cats: CleanupCategory[]) =>
    new Set(cats.filter(c => c.FileCount > 0 && c.Id !== "windowsOld").map(c => c.Id));

/** Windows-owned storage scope of the Maintenance "Reclaim disk space" card. */
export default function DiskCleanupGranular() {
    // Seed from the module-level cache (already populated by the app-startup
    // pre-load, or a previous mount) so a returning/first-time visit renders
    // the live data immediately instead of an empty grid + loading flash.
    const [cats, setCats] = useState<CleanupCategory[]>(() => _diskCleanupCache ?? []);
    const [loading, setLoading] = useState(false);
    const [cleaning, setCleaning] = useState(false);
    const [cleaningAll, setCleaningAll] = useState(false);
    const [showStorageInfo, setShowStorageInfo] = useState(false);
    const [selected, setSelected] = useState<Set<string>>(() => _diskCleanupCache ? defaultSelection(_diskCleanupCache) : new Set());
    const [scheduleMinutes, setScheduleMinutes] = useState<number | null>(null);
    const [scheduleBusy, setScheduleBusy] = useState(false);
    const { invokeDiskCleanup, setAutoEraseSchedule, removeAutoEraseSchedule, getAutoEraseSchedules } = useBackend();
    const { appSettings, patchAppSettings } = useAppState();
    // Scheduling is a paid capability everywhere else (System Cleanup gates it
    // on `hasPaid && !isInvestigator`). This surface writes the SAME
    // Set-AutoEraseSchedule record, so it must honour the same rule or the free
    // tier could register an auto-erase task the paid gate forbids.
    const { hasPaid, isInvestigator } = useEntitlements();
    const schedulesEnabled = hasPaid && !isInvestigator;

    // No cache short-circuit: this is the explicit refresh path (refresh button,
    // post-clean) and must reflect real post-clean state. Results reach
    // `cats`/`selected` through the module subscriber below.
    const refresh = useCallback(async () => {
        setLoading(true);
        const res = await fetchDiskCleanupScan();
        setLoading(false);
        if (!res.success) showError(res.error || "Failed to scan");
    }, []);

    // Subscribe to module-level cache updates so this instance reflects any
    // scan completion — its own, another mount's in-flight one it deduped
    // onto, or the app-startup pre-load resolving after mount.
    useEffect(() => {
        _diskCleanupSubscriber = (freshCats) => {
            setCats(freshCats);
            // Pre-select non-empty safe categories on first load; leaves an
            // existing user selection untouched on subsequent refreshes.
            setSelected(prev => prev.size > 0 ? prev : defaultSelection(freshCats));
        };
        return () => { _diskCleanupSubscriber = null; };
    }, []);

    // Load the existing schedule for the diskCleanup category on mount.
    useEffect(() => {
        (async () => {
            const res = await getAutoEraseSchedules();
            if (!res.success || !res.data?.schedules) return;
            const existing = res.data.schedules.find((entry) => entry.categoryId === "diskCleanup");
            setScheduleMinutes(existing ? existing.intervalMinutes : null);
        })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    useEffect(() => {
        if (cats.length === 0 && !loading) refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    const toggle = (id: string) => setSelected(prev => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id); else next.add(id);
        return next;
    });

    const totalSelectedMb = cats.filter(c => selected.has(c.Id)).reduce((sum, c) => sum + (c.SizeMb || 0), 0);

    // One-shot cleanmgr sweep (Invoke-DiskCleanup). Covers Windows-managed
    // categories the per-row scan does not enumerate — memory dumps, servicing
    // leftovers — and records the run so the ageing labels stay accurate.
    const cleanAll = useCallback(async () => {
        setCleaningAll(true);
        let captured: unknown = null;
        try {
            await runOperation(
                "Disk Cleanup",
                [{ label: "Running disk cleanup...", fn: async () => { const r = await invokeDiskCleanup(); captured = r; return r; } }],
                { mode: "sequential", accent: "neutral" },
            );
            const previous = appSettings?.ideal?.tweaks?.maintenanceRuns?.["cleanup"];
            await patchAppSettings({
                ideal: { tweaks: { maintenanceRuns: {
                    cleanup: { lastRunAt: new Date().toISOString(), runCount: (previous?.runCount ?? 0) + 1 },
                } } },
            });
            showSuccess(formatMaintenanceSuccess("Disk Cleanup", captured));
            refresh();
        } catch {
            // runOperation surfaces the error
        } finally {
            setCleaningAll(false);
        }
    }, [invokeDiskCleanup, refresh, appSettings?.ideal?.tweaks?.maintenanceRuns, patchAppSettings]);

    const clean = useCallback(async () => {
        if (selected.size === 0) return;
        if (selected.has("windowsOld") && !window.confirm(
            "Deleting Windows.old removes the option to roll back to the previous Windows version. Continue?"
        )) return;

        setCleaning(true);
        const res = await executeBackendCommand<{ success?: boolean; freedTotalMb?: number; message?: string }>(
            "Invoke-DiskCleanupCategories",
            { Ids: Array.from(selected).join(",") },
        );
        setCleaning(false);
        if (res.success && res.data?.success) {
            showSuccess(`Freed ${res.data.freedTotalMb?.toFixed(1) ?? "?"} MB`);
            refresh();
        } else {
            showError(res.error || res.data?.message || "Cleanup failed");
        }
    }, [selected, refresh]);

    const changeSchedule = async (value: string) => {
        setScheduleBusy(true);
        try {
            if (value === SCHEDULE_OFF) {
                const res = await removeAutoEraseSchedule("diskCleanup");
                if (!res.success) { showError(res.error || "Failed to remove schedule"); return; }
                setScheduleMinutes(null);
                showSuccess("Disk cleanup schedule removed");
                return;
            }
            const minutes = Math.max(60, Number.parseInt(value, 10));
            const res = await setAutoEraseSchedule("diskCleanup", minutes, false);
            if (!res.success) { showError(res.error || "Failed to set schedule"); return; }
            setScheduleMinutes(minutes);
            showSuccess(`Disk cleanup scheduled every ${formatInterval(minutes)}`);
        } finally {
            setScheduleBusy(false);
        }
    };

    const scanLabel = cats.length ? "Rescan Windows storage" : "Scan Windows storage";

    return (
        <div className="flex flex-col gap-3">
            {/* Tour anchor: the whole action row, so the secondary ring covers
                the Clean button together with the live MB counter it feeds.
                Ringing the counter <span> alone highlighted the number and left
                Clean outside the box (2026-07-26 fix). */}
            <div data-tour="maintenance-disk-cleanup-actions" className="flex flex-wrap items-center gap-2">
                <span className="whitespace-nowrap">
                    <span className="font-mono text-base font-bold text-[var(--accent)]">{totalSelectedMb.toFixed(0)} MB</span>
                    <span className="ml-1 text-xs text-[var(--text-mute)]">selected</span>
                </span>
                <Button variant="danger" size="sm" disabled={cleaning || !selected.size} onClick={() => void clean()}>
                    <Icon icon="clean" />{cleaning ? "Cleaning…" : `Clean ${selected.size} selected`}
                </Button>
                <Button variant="outline" size="sm" disabled={cleaning || cleaningAll} onClick={() => void cleanAll()}
                    title="Run the built-in Windows cleanup sweep (cleanmgr) across every system category">
                    <Icon icon="eraser" />{cleaningAll ? "Cleaning…" : "Clean all"}
                </Button>
                <div className="ml-auto flex items-center gap-2">
                    {schedulesEnabled ? (
                        <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                                <Button size="icon" variant="outline" disabled={scheduleBusy} aria-label="Auto-clean schedule"
                                    title={scheduleMinutes === null ? "Schedule auto-clean" : `Auto-clean every ${formatInterval(scheduleMinutes)}`}>
                                    <Icon icon="time" className={scheduleMinutes === null ? "text-[var(--text-mute)]" : "text-[var(--accent)]"} />
                                </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                                <DropdownMenuLabel>Auto-clean interval</DropdownMenuLabel>
                                <DropdownMenuRadioGroup value={scheduleMinutes === null ? SCHEDULE_OFF : String(scheduleMinutes)} onValueChange={(value) => void changeSchedule(value)}>
                                    <DropdownMenuRadioItem value={SCHEDULE_OFF}>Auto-clean off</DropdownMenuRadioItem>
                                    {SCHEDULE_PRESETS.map(preset => <DropdownMenuRadioItem key={preset.minutes} value={String(preset.minutes)}>{preset.label}</DropdownMenuRadioItem>)}
                                </DropdownMenuRadioGroup>
                            </DropdownMenuContent>
                        </DropdownMenu>
                    ) : !isInvestigator && (
                        <Button size="icon" variant="outline" onClick={() => window.dispatchEvent(new CustomEvent("license-gate-open", { detail: { tab: "buy", featureLabel: "Scheduled Auto-Clean" } }))}
                            title="Scheduled auto-clean is a paid feature">
                            <Icon icon="time" />
                        </Button>
                    )}
                    <Button size="icon" variant="outline" onClick={() => setShowStorageInfo((current) => !current)}
                        title="About Windows storage cleanup" aria-label="About Windows storage cleanup" aria-pressed={showStorageInfo}>
                        <Icon icon="info-sign" />
                    </Button>
                    <Button size="icon" variant="primary" disabled={loading} onClick={() => void refresh()} title={scanLabel} aria-label={scanLabel}>
                        <Icon icon={loading || cats.length ? "refresh" : "search"} className={loading ? "animate-spin" : undefined} />
                    </Button>
                </div>
            </div>
            {showStorageInfo && <p className="text-xs text-[var(--text-mute)]">Windows storage shows safe-to-review system categories. Selected items are only removed when you choose Clean selected.</p>}

            {loading && cats.length === 0 && <div className="flex justify-center py-6"><Spinner size={20} className="text-[var(--accent)]" /></div>}
            <div className="grid gap-2 sm:grid-cols-2">
                {cats.map(c => <CategoryRow key={c.Id} category={c} checked={selected.has(c.Id)} onToggle={() => toggle(c.Id)} />)}
            </div>
        </div>
    );
}

function CategoryRow({ category, checked, onToggle }: { category: CleanupCategory; checked: boolean; onToggle: () => void }) {
    const empty = category.FileCount === 0;
    return (
        <button type="button" disabled={empty} onClick={onToggle}
            className={`flex items-start gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-2 text-left transition-colors hover:bg-[var(--surface-3)] disabled:cursor-default disabled:opacity-50 disabled:hover:bg-[var(--surface-2)] ${checked ? "border-[var(--border-strong)]" : ""}`}>
            <span className={`mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-[var(--r-sm)] border ${checked ? "border-[var(--accent)] bg-[var(--accent)] text-[var(--accent-contrast)]" : "border-[var(--border-strong)]"}`}>
                {checked && <Icon icon="tick" />}
            </span>
            <span className="min-w-0 flex-1">
                <span className="flex items-center gap-1.5 text-xs font-semibold text-[var(--text)]">
                    {category.Id === "windowsOld" && <Icon icon="warning-sign" className="text-[var(--warn)]" />}
                    {category.Label}
                </span>
                <span className="mt-0.5 block font-mono text-[10.5px] text-[var(--text-mute)]">
                    {category.FileCount.toLocaleString()} files · {category.SizeMb.toFixed(1)} MB
                </span>
            </span>
            {category.SizeMb >= 1024 && <Badge tone="warning">{(category.SizeMb / 1024).toFixed(1)} GB</Badge>}
        </button>
    );
}
