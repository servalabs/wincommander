import { useCallback, useEffect, useState } from "react";
import { Badge } from "../../ui/badge";
import { Button } from "../../ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuLabel, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "../../ui/dropdown-menu";
import { Icon } from "../../ui/icon";
import { Spinner } from "../../ui/spinner";
import { CheckboxControl } from "../../ui/bp";
import useBackend, { executeBackendCommand } from "../../../hooks/useBackend";
import useEntitlements from "../../../hooks/useEntitlements";
import { useAppState } from "../../../context/AppContext";
import { runOperation } from "../../../context/OperationContext";
import { showSuccess, showError } from "../../../utils/toast";
import { formatMaintenanceSuccess } from "../../../utils/maintenance";
import { useAppConfirm } from "../../shared/AppConfirmDialog";

interface CleanupCategory {
    Id: string;
    Label: string;
    Path: string;
    Exists: boolean;
    FileCount: number;
    SizeBytes: number;
    SizeMb: number;
}

interface AutoEraseSchedule {
    categoryId: string;
    taskName: string;
    enabled: boolean;
    intervalMinutes: number;
    targetUser: string | null;
    ownerAccount?: string | null;
    lastRun: string | null;
    lastResult: number | null;
}

interface WindowsAccount {
    name: string;
    displayName?: string;
    sid?: string;
    isCurrent?: boolean;
}

/**
 * A scheduled cleanup is per-user. Its task name is only a label: the
 * Scheduler principal (`ownerAccount`) is the identity Windows will actually
 * use when it runs the task. Do not infer coverage from an unsuffixed task
 * name, because that task can belong to another signed-in account.
 */
export function isSignedInAccountCovered(
    entry: Pick<AutoEraseSchedule, "taskName" | "ownerAccount">,
    account: Pick<WindowsAccount, "name" | "displayName" | "sid"> | undefined,
): boolean {
    const owner = entry.ownerAccount?.trim();
    if (!owner || !account) return false;

    return [account.name, account.displayName, account.sid]
        .filter((identity): identity is string => Boolean(identity?.trim()))
        .some((identity) => identity.trim().localeCompare(owner, undefined, { sensitivity: "accent" }) === 0);
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
    const requestConfirm = useAppConfirm();
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
    const [scheduleError, setScheduleError] = useState<string | null>(null);
    const [scheduleEntries, setScheduleEntries] = useState<AutoEraseSchedule[]>([]);
    const [accounts, setAccounts] = useState<WindowsAccount[]>([]);
    const [selectedAccounts, setSelectedAccounts] = useState<Set<string>>(new Set());
    const [isAdmin, setIsAdmin] = useState<boolean | null>(null);
    const {
        invokeDiskCleanup,
        getAutoEraseSchedules,
        getUserProfiles,
        removeMultiUserAutoEraseSchedule,
        setMultiUserAutoEraseSchedule,
    } = useBackend();
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

    const refreshSchedules = useCallback(async () => {
        const res = await getAutoEraseSchedules();
        if (!res.success || !res.data?.schedules) {
            setScheduleError(res.error || "Unable to read auto-clean task status");
            return;
        }
        const entries = res.data.schedules.filter((entry) => entry.categoryId === "diskCleanup") as AutoEraseSchedule[];
        setScheduleEntries(entries);
        const enabled = entries.find((entry) => entry.enabled && entry.intervalMinutes > 0);
        setScheduleMinutes(enabled ? enabled.intervalMinutes : null);
    // useBackend returns stable command references for the component lifetime.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // Load the existing schedule and the accounts that can own its per-user tasks.
    useEffect(() => {
        (async () => {
            const profiles = await getUserProfiles();
            if (profiles.success && profiles.data) {
                const visible = profiles.data.profiles
                    .filter((account) => account.name && account.name !== "Default" && account.name !== "Public")
                    .map((account) => ({
                        name: account.name,
                        displayName: account.displayName,
                        sid: account.sid,
                        isCurrent: account.isCurrent || account.name === profiles.data?.currentUser,
                    }));
                setAccounts(visible);
                setIsAdmin(profiles.data.isAdmin);
                setSelectedAccounts(new Set(visible.filter((account) => account.isCurrent).map((account) => account.name)));
            } else {
                setScheduleError(profiles.error || "Unable to identify the signed-in Windows account");
            }
            await refreshSchedules();
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
        if (selected.has("windowsOld")) {
            const accepted = await requestConfirm({
                title: "Delete Windows.old?",
                description: "Deleting Windows.old removes the option to roll back to the previous Windows version. This cannot be undone.",
                confirmLabel: "Delete Windows.old",
            });
            if (!accepted) return;
        }

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
    }, [requestConfirm, selected, refresh]);

    const changeSchedule = async (value: string) => {
        if (isAdmin === false) {
            setScheduleError("Blocked: needs-elevation. Requires an administrator to create or remove Windows Scheduled Tasks.");
            return;
        }
        const targetUsers = Array.from(selectedAccounts);
        if (targetUsers.length === 0) {
            setScheduleError("Select at least one Windows account for this scheduled cleanup.");
            return;
        }
        setScheduleBusy(true);
        setScheduleError(null);
        try {
            if (value === SCHEDULE_OFF) {
                const res = await removeMultiUserAutoEraseSchedule("diskCleanup", targetUsers);
                if (!res.success) {
                    const message = res.error || "Failed to remove schedule";
                    setScheduleError(message);
                    showError(message);
                    return;
                }
                await refreshSchedules();
                showSuccess(`Disk cleanup schedule removed for ${targetUsers.join(", ")}`);
                return;
            }
            const minutes = Math.max(60, Number.parseInt(value, 10));
            const res = await setMultiUserAutoEraseSchedule("diskCleanup", minutes, targetUsers, false);
            if (!res.success) {
                const message = res.error || "Failed to set schedule";
                setScheduleError(message);
                showError(message);
                return;
            }
            await refreshSchedules();
            showSuccess(`Disk cleanup scheduled every ${formatInterval(minutes)} for ${targetUsers.join(", ")}`);
        } finally {
            setScheduleBusy(false);
        }
    };

    const scanLabel = cats.length ? "Rescan Windows storage" : "Scan Windows storage";

    return (
        <div className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-2">
                <span data-tour="maintenance-disk-cleanup-actions" className="whitespace-nowrap rounded-[var(--r-sm)] px-1 py-0.5">
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
                                <Button size="icon" variant="outline" disabled={scheduleBusy || isAdmin === false} aria-label="Auto-clean schedule"
                                    title={scheduleMinutes === null ? "Schedule auto-clean" : `Auto-clean every ${formatInterval(scheduleMinutes)}`}>
                                    <Icon icon="time" className={scheduleMinutes === null ? "text-[var(--text-mute)]" : "text-[var(--accent)]"} />
                                </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                                <DropdownMenuLabel>Auto-clean interval</DropdownMenuLabel>
                                <div className="border-b border-[var(--border)] px-2 py-2">
                                    <p className="mb-1 text-[10px] font-mono uppercase tracking-wide text-[var(--text-mute)]">Accounts covered</p>
                                    {accounts.map((account) => (
                                        <label key={account.name} className="flex cursor-pointer items-center gap-2 py-1 text-xs">
                                            <CheckboxControl
                                                checked={selectedAccounts.has(account.name)}
                                                ariaLabel={`Schedule cleanup for ${account.displayName ?? account.name}`}
                                                onChange={(event) => setSelectedAccounts((previous) => {
                                                    const next = new Set(previous);
                                                    if (event.currentTarget.checked) next.add(account.name); else next.delete(account.name);
                                                    return next;
                                                })}
                                            />
                                            <span>{account.displayName ?? account.name}{account.isCurrent ? " (signed in)" : ""}</span>
                                        </label>
                                    ))}
                                </div>
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
            {isAdmin === false && (
                <p role="alert" className="text-xs text-[var(--warn)]">Blocked: needs-elevation. Requires an administrator to schedule cleanup for Windows accounts.</p>
            )}
            {scheduleError && <p role="alert" className="text-xs text-[var(--danger)]">{scheduleError}</p>}
            {scheduleEntries.length > 0 && (
                <div className="rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-2 text-xs">
                    <p className="mb-1 font-semibold text-[var(--text)]">Disk cleanup task status</p>
                    {scheduleEntries.map((entry) => {
                        const currentAccount = accounts.find((account) => account.isCurrent);
                        const covered = isSignedInAccountCovered(entry, currentAccount);
                        return <p key={entry.taskName} className="font-mono text-[10.5px] text-[var(--text-mute)]">
                            {entry.taskName} · owner {entry.ownerAccount ?? entry.targetUser ?? "unknown"} · last {entry.lastRun ?? "never"} · result {entry.lastResult ?? "not available"} · {covered ? "signed-in account covered" : "signed-in account not covered"}
                        </p>;
                    })}
                </div>
            )}

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
        <div onClick={() => !empty && onToggle()}
            className={`flex items-start gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-2 text-left transition-colors hover:bg-[var(--surface-3)] ${empty ? "cursor-default opacity-50 hover:bg-[var(--surface-2)]" : "cursor-pointer"} ${checked ? "border-[var(--border-strong)]" : ""}`}>
            <CheckboxControl checked={checked} disabled={empty} ariaLabel={`Select ${category.Label}`} onChange={onToggle} onClick={(event) => event.stopPropagation()} />
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
        </div>
    );
}
