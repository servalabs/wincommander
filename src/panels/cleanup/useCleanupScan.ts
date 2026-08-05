// src/panels/cleanup/useCleanupScan.ts
// Scan orchestration: category state, scan trigger/progress, results, and
// the multi-user (Other User Profiles) viewer. Extracted verbatim from
// src/panels/cleanup/index.tsx — pure move, no behavior change.
import { useEffect, useMemo, useRef, useState } from "react";
import { showSuccess, showError } from "../../utils/toast";
import useBackend from "../../hooks/useBackend";
import { STANDARD_CATEGORIES, DEEP_DFIR_CATEGORIES, VIEW_ONLY_CATEGORIES, ACTION_CATEGORIES, CLEANUP_USABILITY_TIERS, type CleanupCategory, type CleanupUsabilityTier } from "./cleanupCategories";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";

const getSchedulerCategoryId = (categoryId: string): string =>
    [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]
        .find(c => c.id === categoryId)
        ?.schedulerCategoryId ?? categoryId;

// ── Module-level cache ───────────────────────────────────────────────
// Persists loaded card data across panel unmount/remount so navigating
// away and back shows results instantly without re-spawning PowerShell.
// Also used to store results from in-flight scans that complete after unmount.
export type CardData = { count: number; items: string[]; loading: boolean; clearing: boolean; error?: string; raw?: any };
let _cleanupCache: Record<string, CardData> = {};
// Per-other-user scan cache (keyed by profile folder name). Switching back to
// an already-scanned account shows its traces instantly instead of re-running
// the slow offline-hive scan (14-22s) every time. Persists across remounts.
let _otherUserCache: Record<string, Record<string, CardData>> = {};
// Each visible cleanup tab owns a distinct batch. Keeping these IDs (rather
// than a single global "loading all" flag) means an operator can start a scan
// in one tier, move to another tier, and scan that tier without cancelling or
// blocking the first one.
let _scanningBatchIds = new Set<string>();

export function getCleanupScanConcurrency(logicalCores?: number): number {
    const detectedCores = logicalCores ?? (
        typeof navigator !== 'undefined' ? navigator.hardwareConcurrency : undefined
    );
    return Math.max(3, Math.floor((detectedCores || 1) / 2));
}

/** Runs independent cleanup work with the same bounded worker policy as scans. */
export async function runCleanupWorkers<T>(
    items: readonly T[],
    worker: (item: T) => Promise<void>,
    concurrency = getCleanupScanConcurrency(),
): Promise<void> {
    const queue = [...items];
    const takeNext = async (): Promise<void> => {
        const item = queue.shift();
        if (item === undefined) return;
        try {
            await worker(item);
        } finally {
            await takeNext();
        }
    };
    await Promise.all(Array.from(
        { length: Math.min(Math.max(1, concurrency), items.length) },
        takeNext,
    ));
}

// Sentinel "account" for the combined All-Users view (#7).
export const ALL_USERS_KEY = '__all__';
// Must match the allowlist implemented by Get-CleanupSummaryAllUsers and
// Invoke-CleanupClearAllUsers in privacy/cleanup.ps1. Current-user cards use
// their dedicated Get-*/Clear-* handlers and are not limited by this set.
export const MULTI_USER_CLEANUP_IDS = new Set<string>([
    'rdpHistory', 'recentFiles', 'jumpLists', 'psHistory',
    'browserFootprints', 'shellBags', 'execCache', 'netDrives',
    'ntUserTraces', 'notepadState', 'walFiles', 'crashDumps', 'recallDb',
    'webCache', 'thumbnailDb', 'notificationDb', 'activitiesTimeline',
    'rdpBitmapCache',
]);
const HIDDEN_PROFILE_NAMES = new Set([
    "default",
    "default user",
    "defaultuser0",
    "public",
    "all users",
    "sandbox",
    "wdagutilityaccount",
]);
const HIDDEN_PROFILE_FRAGMENTS = [
    "sandbox",
    "codex",
    "defaultuser",
    "wdagutilityaccount",
];

function isRealVisibleProfile(profile: { name?: string; displayName?: string; path?: string }): boolean {
    const name = (profile.name ?? "").trim();
    if (!name) return false;
    const displayName = (profile.displayName ?? "").trim();
    const path = (profile.path ?? "").toLowerCase();
    const normalized = name.toLowerCase();
    if (HIDDEN_PROFILE_NAMES.has(normalized) || HIDDEN_PROFILE_NAMES.has(displayName.toLowerCase())) return false;
    if (HIDDEN_PROFILE_FRAGMENTS.some(fragment => normalized.includes(fragment) || displayName.toLowerCase().includes(fragment) || path.includes(fragment))) return false;
    if (path.endsWith("\\users\\default") || path.endsWith("\\users\\public")) return false;
    return true;
}

// Subscribers: the mounted component registers a setter so module-level
// code can push updates into React state even from background scans.
let _cacheSubscriber: ((cache: Record<string, CardData>) => void) | null = null;
let _scanBatchSubscriber: ((batchIds: Set<string>) => void) | null = null;

export function getCleanupScanBatchId(categories: Pick<CleanupCategory, 'id'>[]): string {
    return categories.map(category => category.id).sort().join('|');
}

function publishScanningBatches() {
    _scanBatchSubscriber?.(new Set(_scanningBatchIds));
}

export function updateCacheEntry(id: string, data: CardData) {
    _cleanupCache[id] = data;
    _cacheSubscriber?.({ ..._cleanupCache });
}

interface CleanupScanOptions {
    schedulesEnabled: boolean;
    entitlementsReady: boolean;
    migrationEnabled: boolean;
}

export function useCleanupScan({ schedulesEnabled, entitlementsReady, migrationEnabled }: CleanupScanOptions) {
    const requestConfirm = useAppConfirm();
    const backend = useBackend();
    const {
        clearPowerShellHistory,
        clearJumpLists,
        clearBrowserFootprints,
        clearPrefetch,
        getUsbHistory,
        clearUsbHistory,
        getDnsCacheEntries,
        flushDnsCache,
        getExecutionCache,
        clearExecutionCache,
        getShellBags,
        clearShellBags,
        getProcessIntelligence,
        clearSRUM,
        getSRUMData,
        clearEventLogs,
        getEventLogSummary,
        clearNTFSJournals,
        getPSHistory,
        getRecentFiles,
        getRDPHistory,
        getConnectivityHistory,
        getJumpLists,
        getBrowserFootprints,
        getPrefetchFiles,
        getShadowCopies,
        getNTFSJournals,
        clearRecentFiles,
        clearRDPHistory,
        clearConnectivityHistory,
        getClipboardHistoryStatus,
        clearClipboard,
        clearShadowCopies,
        getWlanProfiles,
        removeWlanProfile,
        getBluetoothDevices,
        clearBluetoothHistory,
        getNetworkDrives,
        clearNetworkDrives,
        clearAmcache,
        clearNTUserTraces,
        clearNotepadState,
        clearPCADatabase,
        invokeCrashDumpErase,
        invokeVirtualMemoryPurge,

        clearSearchIndex,
        clearPrintSpooler,
        invokeSQLiteWALKiller,
        clearRecallDatabase,
        invokeUnallocatedSpaceErase,
        getAmcacheEntries,
        getRecycleBinInfo,
        clearRecycleBinMetadata,
        getNTUserTraces,
        getNotepadStateFiles,
        getPCAInfo,
        getCrashDumpList,
        getSQLiteWALList,
        getRecallDatabaseInfo,
        getSearchIndexInfo,
        getPrintSpoolerInfo,
        getWebCacheInfo,
        getThumbnailCacheInfo,
        getNotificationDbInfo,
        getBranchCacheInfo,
        getEventTranscriptInfo,
        getActivitiesTimelineInfo,
        getRdpBitmapCacheInfo,
        getServicingLogsInfo,
        getDeviceInstallLogsInfo,
        getUsageTraceLogsInfo,
        getDefenderHistoryInfo,
        clearWebCache,
        clearThumbnailCache,
        clearNotificationDb,
        clearBranchCache,
        clearEventTranscript,
        clearActivitiesTimeline,
        clearRdpBitmapCache,
        clearServicingLogs,
        clearDeviceInstallLogs,
        clearUsageTraceLogs,
        clearDefenderHistory,
        setAutoEraseSchedule,
        removeAutoEraseSchedule,
        getAutoEraseSchedules,
        invokeAutoEraseMigration,
        getUserProfiles,
        invokeCleanupClearAllUsers,
        getCleanupSummaryAllUsers,
        error,
    } = backend;

    // Exclusions apply to every bulk cleanup scope. They let an operator keep
    // a category out of a universal sweep without making that card unavailable.
    // wlanProfiles (Wi-Fi Profiles) and browserFootprints (Browser Audit) are
    // pre-excluded every session so a new user can't lose saved Wi-Fi
    // passwords or browsing footprints to a one-click sweep before they've
    // seen the exclude picker — still removable from the set via that picker.
    const [clearAllExcludes, setClearAllExcludesRaw] = useState<Set<string>>(
        () => new Set(['wlanProfiles', 'browserFootprints'])
    );
    const setClearAllExcludes = setClearAllExcludesRaw;

    // ── Multi-user viewer state ────────────────────────────────────────
    // The panel views exactly ONE user at a time. `availableUsers` lists every
    // real account; `currentUser` is the logged-in operator (the default
    // view); `selectedUser` is whose traces the grid currently shows. Only
    // admins may switch to another account — the backend independently
    // enforces this via Test-IsAdmin, so a non-admin is pinned to themselves.
    // When a non-current user is selected, scopeAware cards are populated from
    // Get-CleanupSummaryAllUsers scanned for that single user.
    const [availableUsers, setAvailableUsers] = useState<Array<{ name: string; displayName?: string; path: string; isCurrent?: boolean }>>([]);
    const [currentUser, setCurrentUser] = useState<string>('');
    const [isAdminUser, setIsAdminUser] = useState<boolean>(false);
    const [selectedUser, setSelectedUser] = useState<string>('');
    const [otherUserDataMap, setOtherUserDataMap] = useState<Record<string, CardData>>({});
    const [otherUserLoading, setOtherUserLoading] = useState(false);

    // ── Combined "All Users" view (#7) ──────────────────────────────────
    // Sums every account's per-category counts into one grid; the detail
    // modal breaks the total back down per user (e.g. alice 200 + bob 300 → 500).
    const [allUsersRaw, setAllUsersRaw] = useState<Array<{
        username: string;
        displayName: string;
        categories: Record<string, { count: number; items: string[]; raw?: unknown }>;
    }>>([]);

    // True when the grid is showing the logged-in operator (the default). All
    // the existing single-user load/clear paths apply only in this mode.
    const isViewingAllUsers = selectedUser === ALL_USERS_KEY;
    const isViewingCurrentUser = !isViewingAllUsers && (selectedUser === '' || selectedUser === currentUser);
    const canSwitchUsers = isAdminUser && availableUsers.length > 1;
    const showAccountPicker = true;
    const accountSelectValue = availableUsers.length > 0
        ? (isViewingAllUsers ? ALL_USERS_KEY : (selectedUser || currentUser))
        : "__loading__";
    const hasMultipleAccountChoices = availableUsers.length > 1;
    // Friendly (possibly renamed) name of the account currently in view, for
    // display. Internal keying still uses the stable folder name (`selectedUser`).
    const selectedDisplay = isViewingAllUsers
        ? 'All users'
        : (availableUsers.find(u => u.name === (selectedUser || currentUser))?.displayName ?? (selectedUser || currentUser));

    // Scan one non-current user's scope-aware traces and project them onto the
    // card grid. One backend call returns every category for that user.
    const loadUserView = async (username: string, categoryIds?: string[]) => {
        setOtherUserLoading(true);
        try {
            const res = await getCleanupSummaryAllUsers(categoryIds ?? [], [username]);
            if (res.success && res.data?.users?.length) {
                const u = res.data.users[0];
                // hiveAvailable=false means registry categories show 0 (hive locked/unavailable).
                // This is silent — not an error the user needs to act on.
                const byId: Record<string, { count: number; items: string[]; raw?: unknown }> = {};
                for (const c of u.categories) {
                    byId[c.id] = {
                        count: c.count,
                        items: c.items ?? [],
                        raw: c.records?.length ? { records: c.records } : undefined,
                    };
                }
                const next: Record<string, CardData> = {};
                const requestedIds = categoryIds?.length ? new Set(categoryIds) : null;
                for (const cat of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]) {
                    if (!cat.scopeAware || !MULTI_USER_CLEANUP_IDS.has(cat.id)) continue;
                    if (requestedIds && !requestedIds.has(cat.id)) continue;
                    const d = byId[cat.id];
                    next[cat.id] = {
                        count: d?.count ?? 0,
                        items: d?.items ?? [],
                        loading: false,
                        clearing: false,
                        raw: d?.raw,
                    };
                }
                const merged = categoryIds?.length
                    ? { ...(_otherUserCache[username] ?? {}), ...next }
                    : next;
                _otherUserCache[username] = merged;
                setOtherUserDataMap(merged);
            } else {
                showError(res.error || `Failed to load traces for ${username}.`);
            }
        } catch (e) {
            showError(`Failed to load traces for ${username}: ${e}`);
        } finally {
            setOtherUserLoading(false);
        }
    };

    // Load EVERY account's scope-aware traces, kept per-user, so the grid can
    // sum them and the detail modal can break the total back down (#7).
    const loadAllUsersView = async () => {
        setOtherUserLoading(true);
        try {
            const res = await getCleanupSummaryAllUsers([], []);
            if (res.success && res.data?.users?.length) {
                setAllUsersRaw(res.data.users.map(u => ({
                    username: u.username,
                    displayName: availableUsers.find(a => a.name === u.username)?.displayName ?? u.username,
                    categories: Object.fromEntries((u.categories ?? []).map(c => [c.id, {
                        count: c.count,
                        items: c.items ?? [],
                        raw: c.records?.length ? { records: c.records } : undefined,
                    }])),
                })));
            } else {
                showError(res.error || 'Failed to load combined user traces.');
            }
        } catch (e) {
            showError(`Failed to load combined user traces: ${e}`);
        } finally {
            setOtherUserLoading(false);
        }
    };

    // Switch the viewed user. Current user restores the normal per-category
    // grid. Another user only changes selection; scans are started explicitly
    // from that user's card/rescan controls so they target that profile only.
    const handleSwitchUser = (username: string) => {
        setSelectedUser(username);
        if (username === ALL_USERS_KEY) {
            setAllUsersRaw([]);
            return;
        }
        if (username && username !== currentUser) {
            const cached = _otherUserCache[username];
            if (cached) {
                setOtherUserDataMap(cached);   // instant — no re-scan
            } else {
                setOtherUserDataMap({});
            }
        }
    };

    // Sum each account's per-category counts (+ collect a per-user breakdown)
    // for the combined "All users" grid.
    const combinedDataMap = useMemo<Record<string, CardData>>(() => {
        const map: Record<string, CardData> = {};
        for (const cat of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]) {
            if (!cat.scopeAware || !MULTI_USER_CLEANUP_IDS.has(cat.id)) continue;
            let total = 0;
            const breakdown: string[] = [];
            for (const u of allUsersRaw) {
                const d = u.categories[cat.id];
                const c = d?.count ?? 0;
                total += c;
                if (c > 0) breakdown.push(`${u.displayName} — ${c}`);
            }
            map[cat.id] = { count: total, items: breakdown, loading: false, clearing: false };
        }
        return map;
    }, [allUsersRaw]);

    // ── Auto-erase scheduler state ───────────────────────────────────
    // schedulesById: categoryId → intervalMinutes for cards with an
    // active Windows Scheduled Task. Pre-populated on panel mount.
    // The set of supported categoryIds is hardcoded in
    // cleanupCategories.ts (SUPPORTED_AUTOERASE_IDS) so the clock icon
    // appears instantly — no async roundtrip just to decide visibility.
    const [schedulesById, setSchedulesById] = useState<Record<string, number>>({});
    const [scheduleBusyId, setScheduleBusyId] = useState<string | null>(null);
    const migrationStarted = useRef(false);

    const refreshSchedules = async () => {
        const res = await getAutoEraseSchedules();
        if (res.success && res.data?.schedules) {
            const map: Record<string, number> = {};
            for (const s of res.data.schedules) {
                // Multi-user tasks use suffixed task names and a targetUser.
                // They must not make the current-profile card look scheduled.
                if (s.targetUser) continue;
                if (s.enabled && s.intervalMinutes > 0) {
                    const uiId = s.categoryId === 'clipboard' ? 'clipboardHistory' : s.categoryId;
                    map[uiId] = s.intervalMinutes;
                }
            }
            setSchedulesById(map);
        }
    };

    useEffect(() => {
        // Populate the user switcher immediately. Schedule migration/hydration
        // runs beside it so the profile selector is present as soon as the
        // panel opens instead of waiting behind cleanup task probes.
        (async () => {
            try {
                const res = await getUserProfiles();
                const data = res.data;
                if (res.success && data) {
                    const realProfiles = (data.profiles ?? []).filter(isRealVisibleProfile);
                    const current =
                        realProfiles.find((u) => u.isCurrent)?.name ??
                        realProfiles.find((u) => u.name === data.currentUser)?.name ??
                        realProfiles[0]?.name ??
                        data.currentUser ??
                        '';
                    setAvailableUsers(realProfiles);
                    setCurrentUser(current);
                    setIsAdminUser(!!data.isAdmin);
                    setSelectedUser((prev) => prev || current);
                }
            } catch {}
        })();
        void refreshSchedules();
        // Stable refs from useBackend's useMemo — safe to omit from deps.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    useEffect(() => {
        if (!entitlementsReady || !schedulesEnabled || !migrationEnabled || migrationStarted.current) return;
        migrationStarted.current = true;
        // KT: migration mutates scheduled tasks and is paid. Running it before
        // entitlement and Pro-install resolution could asynchronously cover
        // an open scheduler with LicenseGate or the Pro installer.
        void (async () => {
            try { await invokeAutoEraseMigration(); } catch {}
            await refreshSchedules();
        })();
        // Stable refs from useBackend's useMemo — safe to omit from deps.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [entitlementsReady, schedulesEnabled, migrationEnabled]);

    const handleSetSchedule = async (categoryId: string, minutes: number, runAsSystem: boolean) => {
        setScheduleBusyId(categoryId);
        try {
            const schedulerId = getSchedulerCategoryId(categoryId);
            const res = await setAutoEraseSchedule(schedulerId, minutes, runAsSystem);
            if (res.success) {
                setSchedulesById(prev => ({ ...prev, [categoryId]: minutes }));
                showSuccess(`Auto-clean scheduled every ${minutes} min for ${categoryId}`);
                return true;
            } else {
                showError(res.error || `Failed to schedule auto-clean for ${categoryId}`);
                return false;
            }
        } finally {
            setScheduleBusyId(null);
        }
    };

    const handleClearSchedule = async (categoryId: string) => {
        setScheduleBusyId(categoryId);
        try {
            const schedulerId = getSchedulerCategoryId(categoryId);
            const res = await removeAutoEraseSchedule(schedulerId);
            if (res.success) {
                setSchedulesById(prev => {
                    const next = { ...prev };
                    delete next[categoryId];
                    return next;
                });
                showSuccess(`Auto-clean turned off for ${categoryId}`);
                return true;
            } else {
                showError(res.error || `Failed to turn off auto-clean for ${categoryId}`);
                return false;
            }
        } finally {
            setScheduleBusyId(null);
        }
    };

    useEffect(() => {
        if (error) showError(error);
    }, [error]);

    // ── Card data (manual load + persistent cache) ─────────────────────
    const buildInitial = (): Record<string, CardData> => {
        const map: Record<string, CardData> = {};
        [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES, ...ACTION_CATEGORIES].forEach(c => {
            map[c.id] = _cleanupCache[c.id] ?? { count: -1, items: [], loading: false, clearing: false };
        });
        return map;
    };
    const [cardDataMap, setCardDataMap] = useState<Record<string, CardData>>(buildInitial);
    const [scanningBatchIds, setScanningBatchIds] = useState<Set<string>>(
        () => new Set(_scanningBatchIds),
    );
    const orderedScanCategories = useMemo(() => {
        const slowIds = new Set([
            'shellBags', 'walFiles',
            'eventLogs', 'searchIndex',
        ]);
        // VIEW_ONLY_CATEGORIES (Process Review + SRUM) render in their own
        // dedicated "System Monitoring — view only" section below; including
        // them here would duplicate the cards in the main scan grid.
        const merged = [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES];
        const fast = merged.filter((category) => !slowIds.has(category.id));
        const slow = merged.filter((category) => slowIds.has(category.id));
        return [...fast, ...slow];
    }, []);
    const allTraceCategories = useMemo(
        () => [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES, ...VIEW_ONLY_CATEGORIES],
        []
    );

    // Summary stats for the cleanup header bar — counts across all scan categories
    // (STANDARD + DEEP_DFIR). VIEW_ONLY are excluded — they're never clearable.
    const summaryStats = useMemo(() => {
        let needsCleaning = 0;
        let clean = 0;
        for (const cat of orderedScanCategories) {
            const d = cardDataMap[cat.id];
            if (!d || d.loading || d.count < 0) continue;
            if (d.count > 0) needsCleaning++;
            else clean++;
        }
        return { needsCleaning, clean };
    }, [cardDataMap, orderedScanCategories]);

    // Subscribe to module-level cache updates (survives unmount/remount).
    // Merge into existing state — the module cache only holds categories that have
    // been loaded at least once; replacing the entire map would erase the default
    // "not loaded" state for every unloaded card and cause them all to render the
    // fallback, which previously made unloaded cards appear to be scanning when
    // a single card (e.g., shellBags) triggered a cache update.
    useEffect(() => {
        _cacheSubscriber = (cache) => setCardDataMap(prev => ({ ...prev, ...cache }));
        _scanBatchSubscriber = (batchIds) => setScanningBatchIds(batchIds);
        // Sync current state on mount
        if (Object.keys(_cleanupCache).length > 0) {
            setCardDataMap(prev => ({ ...prev, ..._cleanupCache }));
        }
        setScanningBatchIds(new Set(_scanningBatchIds));
        return () => { _cacheSubscriber = null; _scanBatchSubscriber = null; };
    }, []);

    // Map of category IDs → backend getter functions (using already-destructured values)
    const getterMap: Record<string, (() => Promise<any>) | undefined> = {
        shellBags: getShellBags,
        usbHistory: getUsbHistory,
        dnsCache: getDnsCacheEntries,
        execCache: getExecutionCache,
        clipboardHistory: getClipboardHistoryStatus,
        wlanProfiles: getWlanProfiles,
        btDevices: getBluetoothDevices,
        netDrives: getNetworkDrives,
        processIntel: getProcessIntelligence,
        eventLogs: getEventLogSummary,
        srumData: getSRUMData,
        psHistory: getPSHistory,
        recentFiles: getRecentFiles,
        rdpHistory: getRDPHistory,
        connectivityHistory: getConnectivityHistory,
        jumpLists: getJumpLists,
        browserFootprints: getBrowserFootprints,
        prefetchFiles: getPrefetchFiles,
        shadowCopies: getShadowCopies,
        ntfsJournals: getNTFSJournals,
        amcache: getAmcacheEntries,
        recycleBin: getRecycleBinInfo,
        ntUserTraces: getNTUserTraces,
        notepadState: getNotepadStateFiles,
        pcaDatabase: getPCAInfo,
        crashDumps: getCrashDumpList,
        searchIndex: getSearchIndexInfo,
        printSpooler: getPrintSpoolerInfo,
        walFiles: getSQLiteWALList,
        recallDb: getRecallDatabaseInfo,
        webCache: getWebCacheInfo,
        thumbnailDb: getThumbnailCacheInfo,
        notificationDb: getNotificationDbInfo,
        branchCache: getBranchCacheInfo,
        eventTranscript: getEventTranscriptInfo,
        activitiesTimeline: getActivitiesTimelineInfo,
        rdpBitmapCache: getRdpBitmapCacheInfo,
        servicingLogs: getServicingLogsInfo,
        deviceInstallLogs: getDeviceInstallLogsInfo,
        usageTraceLogs: getUsageTraceLogsInfo,
        defenderHistory: getDefenderHistoryInfo,
    };

    const clearerMap: Record<string, (() => Promise<any>) | undefined> = {
        shellBags: clearShellBags,
        usbHistory: clearUsbHistory,
        dnsCache: flushDnsCache,
        execCache: clearExecutionCache,
        wlanProfiles: () => removeWlanProfile(''),
        btDevices: clearBluetoothHistory,
        netDrives: clearNetworkDrives,
        eventLogs: clearEventLogs,
        srumData: clearSRUM,
        psHistory: clearPowerShellHistory,
        recentFiles: clearRecentFiles,
        rdpHistory: clearRDPHistory,
        connectivityHistory: clearConnectivityHistory,
        clipboardHistory: clearClipboard,
        jumpLists: clearJumpLists,
        browserFootprints: clearBrowserFootprints,
        prefetchFiles: clearPrefetch,
        shadowCopies: clearShadowCopies,
        ntfsJournals: clearNTFSJournals,
        amcache: clearAmcache,
        recycleBin: clearRecycleBinMetadata,
        ntUserTraces: clearNTUserTraces,
        notepadState: clearNotepadState,
        pcaDatabase: clearPCADatabase,
        crashDumps: invokeCrashDumpErase,
        searchIndex: clearSearchIndex,
        printSpooler: clearPrintSpooler,
        walFiles: invokeSQLiteWALKiller,
        recallDb: clearRecallDatabase,
        webCache: clearWebCache,
        thumbnailDb: clearThumbnailCache,
        notificationDb: clearNotificationDb,
        branchCache: clearBranchCache,
        eventTranscript: clearEventTranscript,
        activitiesTimeline: clearActivitiesTimeline,
        rdpBitmapCache: clearRdpBitmapCache,
        servicingLogs: clearServicingLogs,
        deviceInstallLogs: clearDeviceInstallLogs,
        usageTraceLogs: clearUsageTraceLogs,
        defenderHistory: clearDefenderHistory,
        virtualMemory: invokeVirtualMemoryPurge,
        unallocatedErase: invokeUnallocatedSpaceErase,
        // Force SSD TRIM lives in OsRepairCard (Maintenance's old "Repair & hygiene"
        // tab that used to host it is gone, 2026-07) — not dispatched through this map.
    };

    // Keep the card dispatcher in lockstep with cleanupCategories.ts. Most
    // original categories are listed explicitly above because a few need an
    // argument adapter (notably WLAN), but newly-added no-argument viewer and
    // clearer methods can be resolved safely from their declared backend keys.
    // This also prevents a backend-complete card from silently becoming a
    // no-op when a category is added without duplicating it in both maps.
    const backendNoArg = (methodName: string): (() => Promise<any>) | undefined => {
        const candidate = (backend as unknown as Record<string, unknown>)[methodName];
        return typeof candidate === 'function'
            ? () => (candidate as () => Promise<any>)()
            : undefined;
    };
    for (const cat of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES, ...VIEW_ONLY_CATEGORIES, ...ACTION_CATEGORIES]) {
        if (!getterMap[cat.id] && cat.getDataKey) getterMap[cat.id] = backendNoArg(cat.getDataKey);
        if (!clearerMap[cat.id] && cat.clearDataKey) clearerMap[cat.id] = backendNoArg(cat.clearDataKey);
    }

    // Load a single category — writes to module cache so results survive unmount
    const loadSingleCategory = async (cat: CleanupCategory) => {
        const getter = getterMap[cat.id];
        if (!getter) return;
        updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: -1, items: [] }), loading: true, clearing: false });
        try {
            const res = await getter();
            if (res.success && res.data) {
                const { count, items } = cat.extractPreview(res.data);
                updateCacheEntry(cat.id, { count, items, loading: false, clearing: false, raw: res.data });
            } else {
                updateCacheEntry(cat.id, { count: 0, items: [], loading: false, clearing: false, error: res.error });
            }
        } catch (e) {
            updateCacheEntry(cat.id, { count: 0, items: [], loading: false, clearing: false, error: String(e) });
        }
    };

    // Scan all categories with a sliding concurrency window. Previously this
    // was a strict 4-at-a-time batch that only advanced once every slot in
    // the batch completed — so a single slow scan (shellBags, SRUM, SQLite)
    // would stall the other three spots until it finished. The queue now
    // refills immediately: as soon as one card completes, the next pending
    // category takes its slot, keeping the computed number of tasks in flight
    // times until the queue is empty. If the list is shorter than the computed
    // worker count, every item starts at once.
    const loadCategoryBatch = async (cats: CleanupCategory[], tag: 'standard' | 'dfir') => {
        const batchId = getCleanupScanBatchId(cats);
        // A tab cannot start the same batch twice, but independent tabs can
        // continue scanning in parallel as their category sets are disjoint.
        if (!batchId || _scanningBatchIds.has(batchId)) return;
        _scanningBatchIds.add(batchId);
        publishScanningBatches();
        const concurrency = getCleanupScanConcurrency();
        try {
            await runCleanupWorkers(cats, loadSingleCategory, concurrency);
        } finally {
            _scanningBatchIds.delete(batchId);
            publishScanningBatches();
            // Lets the "Scan All" tour step (do-it-yourself) unlock its Next
            // button once the real scan actually finishes — see tour-cleanup.
            if (tag === 'standard') {
                window.dispatchEvent(new CustomEvent("tour-cleanup-scan-done"));
            }
        }
    };

    const isCategoryBatchScanning = (cats: Pick<CleanupCategory, 'id'>[]) =>
        scanningBatchIds.has(getCleanupScanBatchId(cats));

    // ── Main grid (current user / this machine) ─────────────────────────
    // The main "Advanced Defense & Traces" grid ALWAYS shows the current
    // profile/machine and never changes when another user is selected — the
    // multi-user controls live entirely in the separate "Other User Profiles"
    // sub-section below.

    // Per-card rescan for the current user.
    const handleCardLoad = async (cat: CleanupCategory) => {
        await loadSingleCategory(cat);
    };

    // Clear a category for the CURRENT user (in-process single-user clear).
    // `onDriveWipe` opens the drive-wipe selector dialog (owned by the caller)
    // instead of running the generic confirm+clear flow.
    type ClearResult = 'cleared' | 'reduced' | 'unchanged' | 'failed';

    const clearAndReconcile = async (cat: CleanupCategory): Promise<{
        result: ClearResult;
        before: number;
        after: number;
    }> => {
        const before = Math.max(0, _cleanupCache[cat.id]?.count ?? cardDataMap[cat.id]?.count ?? 0);
        const clearer = clearerMap[cat.id];
        if (!clearer) return { result: 'failed', before, after: before };

        updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: before, items: [] }), clearing: true, loading: false });
        try {
            const res = await clearer();
            await loadSingleCategory(cat);
            const after = Math.max(0, _cleanupCache[cat.id]?.count ?? before);
            if (!res.success && !res.data) return { result: 'failed', before, after };
            if (after === 0) return { result: 'cleared', before, after };
            if (after < before) return { result: 'reduced', before, after };
            return { result: 'unchanged', before, after };
        } catch {
            await loadSingleCategory(cat);
            const after = Math.max(0, _cleanupCache[cat.id]?.count ?? before);
            return { result: 'failed', before, after };
        }
    };

    const handleCardClear = async (cat: CleanupCategory, onDriveWipe?: () => void) => {
        if (cat.id === 'unallocatedErase') {
            onDriveWipe?.();
            return;
        }
        if (cat.confirmMessage) {
            const accepted = await requestConfirm({
                title: `Clear ${cat.label}?`,
                description: cat.confirmMessage,
                confirmLabel: "Clear",
            });
            if (!accepted) return;
        }
        // Background tasks return before the actual drive work completes.
        if (cat.id === 'unallocatedErase' || cat.id === 'virtualMemory') {
            const clearer = clearerMap[cat.id];
            if (!clearer) return;
            updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: 0, items: [] }), clearing: true, loading: false });
            try {
                const res = await clearer();
                if (res.success || res.data) {
                    updateCacheEntry(cat.id, { count: 0, items: [], loading: false, clearing: false });
                    showSuccess(`${cat.label} started in background — this may take 30+ minutes.`);
                } else {
                    showError(res.error || `Failed to start ${cat.label}.`);
                    updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: 0, items: [] }), clearing: false, loading: false });
                }
            } catch (e) {
                showError(`Failed to start ${cat.label}: ${e}`);
                updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: 0, items: [] }), clearing: false, loading: false });
            }
            return;
        }

        const outcome = await clearAndReconcile(cat);
        if (outcome.result === 'cleared') {
            showSuccess(`${cat.label} cleared.`);
        } else if (outcome.result === 'reduced') {
            showSuccess(`${cat.label} reduced from ${outcome.before} to ${outcome.after}.`);
        } else if (outcome.result === 'unchanged') {
            showError(`${cat.label} still reports ${outcome.after} traces after cleanup.`);
        } else {
            showError(`Failed to clear ${cat.label}.`);
        }
    };

    const handleClearCategories = async (
        categories: CleanupCategory[],
        label: string,
        excludedIds = new Set<string>(),
    ) => {
        const withFindings = categories.filter(cat =>
            !cat.actionOnly &&
            (cardDataMap[cat.id]?.count ?? 0) > 0 &&
            clearerMap[cat.id] &&
            !excludedIds.has(cat.id)
        );
        if (!withFindings.length) return;
        const excludedNames = Array.from(excludedIds).map(id => {
            const cat = orderedScanCategories.find(c => c.id === id);
            return cat?.label ?? id;
        });
        const excludeNote = excludedNames.length
            ? ` (excluding ${excludedNames.join(', ')})`
            : '';
        const accepted = await requestConfirm({
            title: `Clear ${label}?`,
            description: `Clear findings across ${withFindings.length} categories${excludeNote}? Each category will be re-scanned after cleanup.`,
            confirmLabel: "Clear categories",
        });
        if (!accepted) return;
        const results = new Map<string, ClearResult>();
        await runCleanupWorkers(withFindings, async cat => {
            results.set(cat.id, (await clearAndReconcile(cat)).result);
        });
        const cleared = withFindings.filter(cat => results.get(cat.id) === 'cleared');
        const reduced = withFindings.filter(cat => results.get(cat.id) === 'reduced');
        const unchanged = withFindings.filter(cat => results.get(cat.id) === 'unchanged');
        const failed = withFindings.filter(cat => results.get(cat.id) === 'failed');
        if (cleared.length || reduced.length) {
            const summary = [
                cleared.length ? `${cleared.length} cleared` : '',
                reduced.length ? `${reduced.length} reduced` : '',
            ].filter(Boolean).join(', ');
            showSuccess(`${label}: ${summary}.`);
        }
        if (unchanged.length || failed.length) {
            const unchangedLabels = unchanged.map(cat => cat.label);
            const failedLabels = failed.map(cat => cat.label);
            const parts = [
                unchangedLabels.length ? `unchanged: ${unchangedLabels.join(', ')}` : '',
                failedLabels.length ? `failed: ${failedLabels.join(', ')}` : '',
            ].filter(Boolean).join('; ');
            showError(`${label}: ${parts}.`);
        }
    };

    const handleClearTier = async (tier: CleanupUsabilityTier, excludedIds = new Set<string>()) => {
        const tierLabel = tier === 'low-impact'
            ? 'low-impact traces'
            : CLEANUP_USABILITY_TIERS.find((item) => item.id === tier)?.label ?? 'this section';
        await handleClearCategories(
            orderedScanCategories.filter(cat => cat.usabilityTier === tier),
            tierLabel,
            excludedIds,
        );
    };
    // Low impact keeps its existing exclusion picker. Other tabs clear only
    // their own eligible findings through the shared tier action above.
    const handleClearAllTraces = async () => handleClearTier('low-impact', clearAllExcludes);
    const handleClearAllCategories = async () =>
        handleClearCategories(orderedScanCategories, 'all cleanup findings', clearAllExcludes);

    // ── Other-users sub-section ─────────────────────────────────────────
    // Clears for the selected OTHER user (or all users) write into the
    // sub-section's own data map, so the main current-user grid is never
    // touched. `targets` undefined = all users.
    const runOtherUserClear = async (cat: CleanupCategory, targets: string[] | undefined) => {
        const prev = otherUserDataMap[cat.id] || { count: 0, items: [] };
        const setCard = (data: CardData) => {
            setOtherUserDataMap(p => ({ ...p, [cat.id]: data }));
            // Keep the per-user cache in sync so switching away and back shows
            // the post-clear state, not stale pre-clear counts.
            if (selectedUser && _otherUserCache[selectedUser]) {
                _otherUserCache[selectedUser] = { ..._otherUserCache[selectedUser], [cat.id]: data };
            }
        };
        // Keep the count while clearing so the card footer + "Clearing..." show.
        setCard({ count: prev.count ?? 0, items: prev.items ?? [], loading: false, clearing: true });
        try {
            const res = await invokeCleanupClearAllUsers([cat.id], targets);
            if (res.success) {
                const d = res.data;
                if (d && d.partial > 0) {
                    // Partial clear — a logged-in user's registry hive was locked
                    // so some traces remain. Re-scan for the TRUE remaining count
                    // instead of hard-coding 0, which previously wrote a false
                    // "cleared" (count 0) into the card and the per-user cache.
                    if (targets && targets.length) {
                        await loadUserView(targets[0], [cat.id]);
                    } else {
                        await loadAllUsersView();
                    }
                } else {
                    setCard({ count: 0, items: [], loading: false, clearing: false });
                }
                const partialNote = d && d.partial > 0
                    ? ` (${d.partial} logged-in user${d.partial !== 1 ? 's' : ''} partial — registry skipped)`
                    : '';
                showSuccess(`${cat.label} cleared for ${d?.cleaned ?? 0} user${(d?.cleaned ?? 0) !== 1 ? 's' : ''}${partialNote}.`);
            } else {
                showError(res.error || `Failed to clear ${cat.label}.`);
                setCard({ count: prev.count ?? 0, items: prev.items ?? [], loading: false, clearing: false });
            }
        } catch (e) {
            showError(`Failed to clear ${cat.label}: ${e}`);
            setCard({ count: prev.count ?? 0, items: prev.items ?? [], loading: false, clearing: false });
        }
    };

    // "Clear for {selected user}" in the sub-section.
    const handleOtherUserClear = async (cat: CleanupCategory) => {
        if (!selectedUser) return;
        const accepted = await requestConfirm({
            title: `Clear ${cat.label}?`,
            description: `Clear ${cat.label} for ${selectedDisplay}? The selected profile will be re-scanned afterward.`,
            confirmLabel: "Clear profile",
        });
        if (!accepted) return;
        await runOtherUserClear(cat, [selectedUser]);
    };

    // "Clear for all users" in the sub-section (admin-only, scopeAware-only).
    const handleCardClearAllUsers = async (cat: CleanupCategory) => {
        const accepted = await requestConfirm({
            title: `Clear ${cat.label} for all users?`,
            description: `Clear ${cat.label} for all ${availableUsers.length} user profiles on this machine? This cannot be undone.`,
            confirmLabel: "Clear all users",
        });
        if (!accepted) return;
        await runOtherUserClear(cat, undefined);
    };

    return {
        cardDataMap,
        isCategoryBatchScanning,
        orderedScanCategories,
        allTraceCategories,
        summaryStats,
        loadCategoryBatch,
        handleCardLoad,
        handleCardClear,
        handleClearAllTraces,
        handleClearAllCategories,
        handleClearTier,
        clearAllExcludes,
        setClearAllExcludes,
        availableUsers,
        currentUser,
        isAdminUser,
        selectedUser,
        otherUserDataMap,
        otherUserLoading,
        allUsersRaw,
        combinedDataMap,
        isViewingAllUsers,
        isViewingCurrentUser,
        canSwitchUsers,
        showAccountPicker,
        accountSelectValue,
        hasMultipleAccountChoices,
        selectedDisplay,
        loadUserView,
        loadAllUsersView,
        handleSwitchUser,
        handleOtherUserClear,
        handleCardClearAllUsers,
        schedulesById,
        scheduleBusyId,
        handleSetSchedule,
        handleClearSchedule,
    };
}

