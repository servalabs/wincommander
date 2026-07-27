// src/panels/cleanup/useCleanupScan.ts
// Scan orchestration: category state, scan trigger/progress, results, and
// the multi-user (Other User Profiles) viewer. Extracted verbatim from
// src/panels/cleanup/index.tsx — pure move, no behavior change.
import { useEffect, useMemo, useRef, useState } from "react";
import { showSuccess, showError } from "../../utils/toast";
import useBackend from "../../hooks/useBackend";
import { STANDARD_CATEGORIES, DEEP_DFIR_CATEGORIES, VIEW_ONLY_CATEGORIES, ACTION_CATEGORIES, isLowImpactCategory, type CleanupCategory } from "./cleanupCategories";

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
let _scanningTag: 'standard' | 'dfir' | null = null;

export function getCleanupScanConcurrency(logicalCores?: number): number {
    const detectedCores = logicalCores ?? (
        typeof navigator !== 'undefined' ? navigator.hardwareConcurrency : undefined
    );
    return Math.max(3, Math.floor((detectedCores || 1) / 2));
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
let _scanTagSubscriber: ((tag: 'standard' | 'dfir' | null) => void) | null = null;

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

    // Bulk cleanup only targets low-impact categories. These exclusions offer
    // a further opt-out without allowing a bulk action to reach higher tiers.
    const [clearAllExcludes, setClearAllExcludesRaw] = useState<Set<string>>(new Set());
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
    const [allUsersRaw, setAllUsersRaw] = useState<Array<{ username: string; displayName: string; categories: Record<string, { count: number; items: string[] }> }>>([]);

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
                const byId: Record<string, { count: number; items: string[] }> = {};
                for (const c of u.categories) byId[c.id] = { count: c.count, items: c.items ?? [] };
                const next: Record<string, CardData> = {};
                const requestedIds = categoryIds?.length ? new Set(categoryIds) : null;
                for (const cat of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]) {
                    if (!cat.scopeAware || !MULTI_USER_CLEANUP_IDS.has(cat.id)) continue;
                    if (requestedIds && !requestedIds.has(cat.id)) continue;
                    const d = byId[cat.id];
                    next[cat.id] = { count: d?.count ?? 0, items: d?.items ?? [], loading: false, clearing: false };
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
                    categories: Object.fromEntries((u.categories ?? []).map(c => [c.id, { count: c.count, items: c.items ?? [] }])),
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
    const [loadingAll, setLoadingAll] = useState<'standard' | 'dfir' | null>(_scanningTag);
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
        _scanTagSubscriber = (tag) => setLoadingAll(tag);
        // Sync current state on mount
        if (Object.keys(_cleanupCache).length > 0) {
            setCardDataMap(prev => ({ ...prev, ..._cleanupCache }));
        }
        setLoadingAll(_scanningTag);
        return () => { _cacheSubscriber = null; _scanTagSubscriber = null; };
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
        // Force SSD TRIM moved to the Maintenance panel's Repair & Hygiene tab (2026-07).
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
        _scanningTag = tag;
        _scanTagSubscriber?.(tag);
        const concurrency = getCleanupScanConcurrency();
        const queue = [...cats];
        const workers: Promise<void>[] = [];
        const spawn = (): Promise<void> => {
            const next = queue.shift();
            if (!next) return Promise.resolve();
            return loadSingleCategory(next).then(spawn);
        };
        for (let i = 0; i < Math.min(concurrency, cats.length); i++) {
            workers.push(spawn());
        }
        await Promise.allSettled(workers);
        _scanningTag = null;
        _scanTagSubscriber?.(null);
        // Lets the "Scan All" tour step (do-it-yourself) unlock its Next
        // button once the real scan actually finishes — see tour-cleanup.
        if (tag === 'standard') {
            window.dispatchEvent(new CustomEvent("tour-cleanup-scan-done"));
        }
    };

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
    const handleCardClear = async (cat: CleanupCategory, onDriveWipe?: () => void) => {
        if (cat.id === 'unallocatedErase') {
            onDriveWipe?.();
            return;
        }
        if (cat.confirmMessage && !window.confirm(cat.confirmMessage)) return;
        const clearer = clearerMap[cat.id];
        if (!clearer) return;

        updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: 0, items: [] }), clearing: true, loading: false });
        try {
            const res = await clearer();
            if (res.success || res.data) {
                updateCacheEntry(cat.id, { count: 0, items: [], loading: false, clearing: false });
                await loadSingleCategory(cat);
                // Background tasks (Free Space Cleanup, Virtual Memory) return a PID
                // immediately — the actual work runs for minutes. Reflect this in the toast.
                const isBackground = cat.id === 'unallocatedErase' || cat.id === 'virtualMemory';
                if (isBackground) {
                    showSuccess(`${cat.label} started in background — this may take 30+ minutes.`);
                } else {
                    showSuccess(`${cat.label} cleared.`);
                }
            } else {
                showError(res.error || `Failed to clear ${cat.label}.`);
                updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: 0, items: [] }), clearing: false, loading: false });
            }
        } catch (e) {
            showError(`Failed to clear ${cat.label}: ${e}`);
            updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: 0, items: [] }), clearing: false, loading: false });
        }
    };

    // "Clear Low-Impact" — single confirm, then clears only low-impact
    // categories with findings. Higher-impact categories always stay per-card.
    const handleClearAllTraces = async () => {
        const withFindings = orderedScanCategories.filter(cat =>
            !cat.actionOnly &&
            isLowImpactCategory(cat) &&
            (cardDataMap[cat.id]?.count ?? 0) > 0 &&
            clearerMap[cat.id] &&
            !clearAllExcludes.has(cat.id)
        );
        if (!withFindings.length) return;
        const excludedNames = Array.from(clearAllExcludes).map(id => {
            const cat = orderedScanCategories.find(c => c.id === id);
            return cat?.label ?? id;
        });
        const excludeNote = excludedNames.length
            ? ` (excluding ${excludedNames.join(', ')})`
            : '';
        if (!window.confirm(
            `Clear low-impact traces across ${withFindings.length} categories${excludeNote}?`
        )) return;
        const stillHasData: string[] = [];
        for (const cat of withFindings) {
            const clearer = clearerMap[cat.id];
            if (!clearer) continue;
            updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] ?? { count: 0, items: [] }), clearing: true, loading: false });
            try {
                const res = await clearer();
                if (res.success || res.data) {
                    // Reconcile against a fresh Get-* scan instead of assuming the
                    // clear removed everything — mirrors handleCardClear so a
                    // backend ok:false (no-op) surfaces as a residual count here too.
                    await loadSingleCategory(cat);
                    if ((_cleanupCache[cat.id]?.count ?? 0) > 0) {
                        stillHasData.push(cat.label);
                    }
                } else {
                    await loadSingleCategory(cat);
                    stillHasData.push(cat.label);
                }
            } catch {
                await loadSingleCategory(cat);
                stillHasData.push(cat.label);
            }
        }
        if (stillHasData.length) {
            showError(`Some traces could not be cleared: ${stillHasData.join(', ')}.`);
        } else {
            showSuccess('Low-impact traces cleared.');
        }
    };

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
        if (!window.confirm(`Clear ${cat.label} for ${selectedDisplay}?`)) return;
        await runOtherUserClear(cat, [selectedUser]);
    };

    // "Clear for all users" in the sub-section (admin-only, scopeAware-only).
    const handleCardClearAllUsers = async (cat: CleanupCategory) => {
        if (!window.confirm(`Clear ${cat.label} for ALL ${availableUsers.length} users on this machine? This cannot be undone.`)) return;
        await runOtherUserClear(cat, undefined);
    };

    return {
        cardDataMap,
        loadingAll,
        orderedScanCategories,
        allTraceCategories,
        summaryStats,
        loadCategoryBatch,
        handleCardLoad,
        handleCardClear,
        handleClearAllTraces,
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

