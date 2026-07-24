import { useEffect, useRef, useState, useCallback } from "react";
import { Button, ButtonGroup, Checkbox, Spinner, Icon, Popover, Menu, MenuItem, MenuDivider, InputGroup } from "@/components/ui/bp";
import SectionCard from "../../shared/SectionCard";
import useBackend, { executeBackendCommand } from "../../../hooks/useBackend";
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

const SCHEDULE_PRESETS = [
    { minutes: 60,    label: "Every hour" },
    { minutes: 360,   label: "Every 6 hours" },
    { minutes: 720,   label: "Every 12 hours" },
    { minutes: 1440,  label: "Daily" },
    { minutes: 10080, label: "Weekly" },
];

function formatInterval(minutes: number): string {
    if (minutes < 60) return `${minutes}m`;
    if (minutes < 1440) return `${minutes / 60}h`;
    if (minutes < 10080) return `${minutes / 1440}d`;
    return `${minutes / 10080}w`;
}

function containWheelInScroller(e: React.WheelEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    const canScroll = el.scrollHeight > el.clientHeight;
    if (!canScroll) return;

    const atTop = el.scrollTop <= 0;
    const atBottom = Math.ceil(el.scrollTop + el.clientHeight) >= el.scrollHeight;
    const movingUp = e.deltaY < 0;
    const movingDown = e.deltaY > 0;

    if ((movingUp && !atTop) || (movingDown && !atBottom)) {
        e.stopPropagation();
    }
}

// ── Module-level cache ───────────────────────────────────────────────
// Persists the scan result across DiskCleanupGranular mount/unmount so
// navigating away and back shows the "MB selected" counter instantly, and
// lets an app-startup pre-load (see preloadDiskCleanupScan, wired from
// App.tsx right after splash) populate it before the user ever opens
// Secure Storage — no loading flash on first visit. Mirrors the
// cache/subscriber convention used by src/panels/cleanup/useCleanupScan.ts
// (_cleanupCache / updateCacheEntry).
let _diskCleanupCache: CleanupCategory[] | null = null;
let _diskCleanupInFlight: Promise<DiskCleanupScanResult> | null = null;
let _diskCleanupSubscriber: ((cats: CleanupCategory[]) => void) | null = null;

export type DiskCleanupScanResult =
    | { success: true; categories: CleanupCategory[] }
    | { success: false; error?: string };

// Standalone scan fetch, independent of any mounted component — callable
// both from DiskCleanupGranular's own refresh() and from the app-startup
// pre-load below. Concurrent callers share one in-flight PowerShell scan
// (Get-DiskCleanupScan walks Temp/Windows Update cache/Prefetch/
// Windows.old/etc, which can take a few seconds — not cheap) instead of
// spawning parallel ones. Writes the result into the module cache and
// notifies whichever instance (if any) is currently mounted/subscribed.
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

// Fire-and-forget app-startup pre-load. Called once, early in the app
// lifecycle (App.tsx, right after splash) so the scan is already in
// flight — or already cached — by the time the user navigates to Secure
// Storage. Non-blocking: never awaited by the caller. Swallows failures
// silently; a failed pre-load just leaves the panel to scan normally on
// first open, same as before this change.
export function preloadDiskCleanupScan(): void {
    if (_diskCleanupCache) return; // already populated by an earlier call
    fetchDiskCleanupScan().catch(() => {});
}

export default function DiskCleanupGranular() {
    // Dedicated Cleanup-panel section. Open by default so disk cleanup is a
    // first-class surface rather than feeling like a pop-up/hidden utility.
    const [isOpen, setIsOpen] = useState(true);
    const cardRef = useRef<HTMLDivElement>(null);
    // Seed from the module-level cache (already populated by the app-startup
    // pre-load, or a previous mount) so a returning/first-time visit renders
    // the live data immediately instead of an empty grid + loading flash.
    const [cats, setCats] = useState<CleanupCategory[]>(() => _diskCleanupCache ?? []);
    const [loading, setLoading] = useState(false);
    const [cleaning, setCleaning] = useState(false);
    const [cleaningAll, setCleaningAll] = useState(false);
    const [selected, setSelected] = useState<Set<string>>(() => _diskCleanupCache
        ? new Set(_diskCleanupCache.filter(c => c.FileCount > 0 && c.Id !== "windowsOld").map(c => c.Id))
        : new Set());
    const [scheduleMinutes, setScheduleMinutes] = useState<number | null>(null);
    const [scheduleBusy, setScheduleBusy] = useState(false);
    const [customMinutes, setCustomMinutes] = useState("");
    const { invokeDiskCleanup, setAutoEraseSchedule, removeAutoEraseSchedule, getAutoEraseSchedules } = useBackend();
    const { appSettings, patchAppSettings } = useAppState();

    // Always performs a fresh scan (no cache short-circuit here) — this is
    // the explicit refresh path used by the refresh button and after a
    // clean, and must reflect the real post-clean state. Result propagation
    // into `cats`/`selected` happens via the module-level subscriber below,
    // which fetchDiskCleanupScan() notifies on every successful scan
    // regardless of which mounted instance (or the startup pre-load)
    // triggered it.
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
            setSelected(prev => prev.size > 0 ? prev : new Set(
                freshCats.filter(c => c.FileCount > 0 && c.Id !== "windowsOld").map(c => c.Id)
            ));
        };
        return () => { _diskCleanupSubscriber = null; };
    }, []);

    // Load existing schedule for the diskCleanup category on mount.
    useEffect(() => {
        (async () => {
            const res = await getAutoEraseSchedules();
            if (res.success && res.data?.schedules) {
                const s = res.data.schedules.find((x: any) => x.categoryId === 'diskCleanup');
                setScheduleMinutes(s ? s.intervalMinutes : null);
            }
        })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    useEffect(() => {
        if (isOpen && cats.length === 0 && !loading) refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isOpen]);

    // Fired by the dashboard "≈ X to clean" button to jump directly here.
    useEffect(() => {
        const handler = () => {
            setIsOpen(true);
            // Refresh scan data if not yet loaded (e.g. first navigation to vault).
            if (cats.length === 0 && !loading) refresh();
            setTimeout(() => cardRef.current?.scrollIntoView({ behavior: "smooth", block: "start" }), 120);
        };
        window.addEventListener("open-disk-cleanup", handler);
        return () => window.removeEventListener("open-disk-cleanup", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [cats.length, loading]);

    const toggle = (id: string) => setSelected(prev => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id); else next.add(id);
        return next;
    });

    const totalSelectedMb = cats
        .filter(c => selected.has(c.Id))
        .reduce((s, c) => s + (c.SizeMb || 0), 0);

    // "Clean All" — the one-shot disk-cleanup that used to live in the
    // System Maintenance card. Calls Invoke-DiskCleanup (tweaks/maintenance)
    // and records the run timestamp so the maintenance ageing UI still
    // works. The per-category controls below are the granular flow.
    const cleanAll = useCallback(async () => {
        setCleaningAll(true);
        let captured: any = null;
        const wrapped = async () => {
            const r = await invokeDiskCleanup();
            captured = r;
            return r;
        };
        try {
            await runOperation(
                "Disk Cleanup",
                [{ label: "Running disk cleanup...", fn: wrapped }],
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

    const handleSetSchedule = async (minutes: number) => {
        setScheduleBusy(true);
        try {
            const res = await setAutoEraseSchedule('diskCleanup', minutes, false);
            if (res.success) {
                setScheduleMinutes(minutes);
                showSuccess(`Disk cleanup scheduled every ${formatInterval(minutes)}`);
            } else {
                showError(res.error || "Failed to set schedule");
            }
        } finally {
            setScheduleBusy(false);
        }
    };

    const handleRemoveSchedule = async () => {
        setScheduleBusy(true);
        try {
            const res = await removeAutoEraseSchedule('diskCleanup');
            if (res.success) {
                setScheduleMinutes(null);
                showSuccess("Disk cleanup schedule removed");
            } else {
                showError(res.error || "Failed to remove schedule");
            }
        } finally {
            setScheduleBusy(false);
        }
    };

    const isScheduled = scheduleMinutes !== null;

    return (
        <div ref={cardRef}>
        <SectionCard
            title="Disk Clean-Up"
            icon="trash"
            className="disk-cleanup-section"
            headerRight={
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                    {/* Tour anchor: the live reclaimable-MB counter specifically,
                        not the whole card — this is the number the tour step is
                        actually pointing at (2026-07-10 fix). */}
                    <span className="font-mono" data-tour="maintenance-disk-cleanup-mb" style={{ fontSize: 11, fontWeight: 600, color: 'var(--color-text-muted)', whiteSpace: 'nowrap' }}>
                        {`${totalSelectedMb.toFixed(0)} MB selected`}
                    </span>

                    {/* Split button: Clean (primary) + chevron dropdown → Windows Cleanup */}
                    <ButtonGroup>
                        <Button small intent="danger" icon="clean" loading={cleaning} disabled={selected.size === 0}
                            onClick={(e) => { e.stopPropagation(); clean(); }}>
                            Clean
                        </Button>
                        <Popover
                            minimal
                            placement="bottom-end"
                            popoverClassName="disk-cleanup-popover"
                            content={
                                <Menu>
                                    <MenuItem icon="eraser" text="Windows Cleanup" labelElement={<span style={{ fontSize: 9, opacity: 0.6 }}>system-wide</span>} disabled={cleaningAll} onClick={() => cleanAll()} />
                                </Menu>
                            }
                            renderTarget={({ ref, isOpen: _io, onClick: popoverClick, ...ariaProps }) => (
                                <span ref={ref as React.Ref<HTMLSpanElement>}>
                                    <Button small intent="danger" icon="chevron-down" loading={cleaningAll}
                                        disabled={cleaning || cleaningAll}
                                        {...ariaProps}
                                        onClick={(e) => { e.stopPropagation(); (popoverClick as React.MouseEventHandler)?.(e); }} />
                                </span>
                            )}
                        />
                    </ButtonGroup>

                    {/* Schedule — icon only; green tint signals an active schedule */}
                    <Popover
                        minimal
                        placement="bottom-end"
                        interactionKind="click"
                        popoverClassName="disk-cleanup-popover"
                        content={
                            <Menu>
                                <MenuDivider title={isScheduled ? `Currently every ${formatInterval(scheduleMinutes!)}` : "Auto-run interval"} />
                                {SCHEDULE_PRESETS.map(p => (
                                    <MenuItem
                                        key={p.minutes}
                                        icon={isScheduled && scheduleMinutes === p.minutes ? "tick" : "blank"}
                                        text={p.label}
                                        disabled={scheduleBusy}
                                        onClick={() => handleSetSchedule(p.minutes)}
                                    />
                                ))}
                                <MenuDivider />
                                <li className="bp5-menu-item-custom" style={{ padding: "6px 8px" }}>
                                    <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
                                        <InputGroup
                                            small
                                            type="number"
                                            placeholder="Custom (min 60)"
                                            value={customMinutes}
                                            onChange={(e) => setCustomMinutes(e.target.value)}
                                            style={{ width: 140 }}
                                        />
                                        <Button
                                            small minimal icon="tick"
                                            disabled={scheduleBusy || !customMinutes}
                                            onClick={() => {
                                                const n = parseInt(customMinutes, 10);
                                                if (!Number.isFinite(n)) return;
                                                handleSetSchedule(Math.max(60, n));
                                                setCustomMinutes("");
                                            }}
                                        />
                                    </div>
                                    <div style={{ fontSize: 9, color: "var(--color-text-muted)", marginTop: 4 }}>minutes · min 60</div>
                                </li>
                                {isScheduled && (
                                    <>
                                        <MenuDivider />
                                        <MenuItem icon="disable" text="Remove schedule" intent="danger" disabled={scheduleBusy} onClick={handleRemoveSchedule} />
                                    </>
                                )}
                            </Menu>
                        }
                        renderTarget={({ ref, isOpen: _io, onClick: popoverClick, ...ariaProps }) => (
                            <span ref={ref as React.Ref<HTMLSpanElement>}>
                                <Button
                                    small minimal icon="time"
                                    {...ariaProps}
                                    onClick={(e) => { e.stopPropagation(); (popoverClick as React.MouseEventHandler)?.(e); }}
                                    style={isScheduled ? { background: 'rgba(72,187,120,0.13)', color: '#48bb78' } : undefined}
                                    title={isScheduled ? `Auto-clean every ${formatInterval(scheduleMinutes!)} — needs Administrator` : 'Schedule auto disk cleanup (needs Administrator)'}
                                />
                            </span>
                        )}
                    />

                    <Button small minimal icon="refresh" loading={loading}
                        onClick={(e) => { e.stopPropagation(); refresh(); }} />
                </div>
            }
        >
            {loading && cats.length === 0 && <Spinner size={20} />}
            <div className="disk-cleanup-category-scroll custom-scrollbar" onWheel={containWheelInScroller}>
            <div className="disk-cleanup-category-grid">
                {cats.map(c => (
                    <div key={c.Id}
                        style={{
                            display: "flex", alignItems: "center", gap: 8,
                            padding: "8px 10px", borderRadius: 4,
                            background: "var(--color-bg-elevated, rgba(255,255,255,0.02))",
                            opacity: c.FileCount === 0 ? 0.5 : 1,
                        }}>
                        <Checkbox
                            checked={selected.has(c.Id)}
                            disabled={c.FileCount === 0}
                            onChange={() => toggle(c.Id)}
                            style={{ marginBottom: 0 }}
                        />
                        <div style={{ flex: 1, minWidth: 0 }}>
                            <div style={{ fontWeight: 600, fontSize: 12, display: "flex", alignItems: "center", gap: 6 }}>
                                {c.Id === "windowsOld" && <Icon icon="warning-sign" size={11} intent="warning" />}
                                {c.Label}
                            </div>
                            <div style={{ fontSize: 10, color: "var(--color-text-muted)" }}>
                                {c.FileCount} files · {c.SizeMb.toFixed(1)} MB
                            </div>
                        </div>
                    </div>
                ))}
            </div>
            </div>
        </SectionCard>
        </div>
    );
}
