// src/panels/cleanup/useCleanupScan.ts
// Scan orchestration: category state, scan trigger/progress, results, and
// the multi-user (Other User Profiles) viewer. Extracted verbatim from
// src/panels/cleanup/index.tsx — pure move, no behavior change.
import { useEffect, useMemo, useRef, useState } from "react";
import { showSuccess, showError } from "../../utils/toast";
import useBackend from "../../hooks/useBackend";
import { usePatchSettings, useSettingsQuery } from "../../hooks/queries/useSettingsQuery";
import { STANDARD_CATEGORIES, DEEP_DFIR_CATEGORIES, VIEW_ONLY_CATEGORIES, ACTION_CATEGORIES, SCHEDULABLE_CATEGORIES, type CleanupCategory } from "./cleanupCategories";
import { getAutoScheduleInterval } from "./autoSchedulePolicy";
import { createCleanupScheduleReadGate } from "./cleanupScheduleReadGate";
import { canAutoSetCleanupSchedules, createCleanupScheduleMigrationGate, isVerifiedCleanupMigration, requiresCleanupTaskRepair } from "./cleanupScheduleMigrationGate";
import { isVerifiedIndividualSchedule, isVerifiedManagedSchedule, readCleanupScheduleSnapshot, type CleanupScheduleScope, type CleanupScheduleRow } from "./cleanupScheduleReadback";
import { DEFAULT_BULK_CLEAR_EXCLUDES, isDefaultExcludedCleanup, isAutomaticCleanupExcluded, requestCleanupManualAuthorization, getCleanupManualWarning, getCleanupAutoScheduleConfirmation, isCleanupCommandSuccessful } from "./cleanupManualPolicy";
export { DEFAULT_BULK_CLEAR_EXCLUDES, isDefaultExcludedCleanup } from "./cleanupManualPolicy";
import { readCleanupScanResult, readCleanupUserSummary } from "./cleanupScanResult";
import { cleanupProfileKey, isVisibleCleanupProfile, readCleanupProfileInventory, resolveCurrentCleanupProfile } from "./cleanupProfileIdentity";
import { createCleanupProfileReadGate } from "./cleanupProfileReadGate";
import { OTHER_ACCOUNT_READ_ONLY_IDS } from "./cleanupOtherAccountScheduleOnly";
import { createCleanupVerification, retainCleanupVerification, type CleanupVerification } from "./cleanupVerification";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";
import {
    invalidateDiskCleanupScheduleStatus,
    loadDiskCleanupSchedules,
} from "../maintenance/diskCleanupScheduleState";

const getSchedulerCategoryId = (categoryId: string): string =>
    [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]
        .find(c => c.id === categoryId)
        ?.schedulerCategoryId ?? categoryId;

const SCHEDULE_CACHE_KEY = "wincommander.cleanup.schedule-cache.v1";
// A machine-level SSD/NVMe ReTrim task accompanies bulk scheduled wipes.  It
// is tracked with the bulk schedules so turning the control off removes it;
// no cleanup card reads this synthetic id.
const AUTO_SCHEDULE_TRIM_ID = "maintenanceTrim";
const AUTO_SCHEDULE_TRIM_INTERVAL_MINUTES = 1440;
const CLEANUP_SCHEDULE_IDS = new Set([...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES].map(category => category.id).concat(AUTO_SCHEDULE_TRIM_ID));

function readScheduleCache(): Record<string, number> {
    if (typeof window === "undefined") return {};
    try {
        const cached = JSON.parse(window.localStorage.getItem(SCHEDULE_CACHE_KEY) ?? "{}") as { schedules?: unknown };
        if (!cached.schedules || typeof cached.schedules !== "object" || Array.isArray(cached.schedules)) return {};
        const schedules: Record<string, number> = {};
        for (const [id, minutes] of Object.entries(cached.schedules as Record<string, unknown>)) {
            if (typeof minutes === "number" && Number.isFinite(minutes) && minutes > 0) schedules[id] = minutes;
        }
        return schedules;
    } catch {
        return {};
    }
}

function writeScheduleCache(schedules: Record<string, number>): void {
    if (typeof window === "undefined") return;
    try { window.localStorage.setItem(SCHEDULE_CACHE_KEY, JSON.stringify({ schedules, checkedAt: new Date().toISOString() })); } catch {}
}

// ── Module-level cache ───────────────────────────────────────────────
// Persists loaded card data across panel unmount/remount so navigating
// away and back shows results instantly without re-spawning PowerShell.
// Also used to store results from in-flight scans that complete after unmount.
export type CardData = { count: number; items: string[]; loading: boolean; clearing: boolean; error?: string; raw?: any; verification?: CleanupVerification };
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

export type CleanupClearResult = 'cleared' | 'reduced' | 'reset' | 'unchanged' | 'deferred' | 'failed';

export interface CleanupClearReconciliation {
    result: CleanupClearResult;
    removed: number;
}

export interface AutoSetScheduleSummary {
    created: number;
    alreadyConfigured: number;
    disabledExisting: number;
    disabled: number;
    skipped: number;
    failed: number;
}

export const formatAutoSetScheduleSummary = (summary: AutoSetScheduleSummary) =>
    summary.disabled > 0
        ? `Scheduled wipes: ${summary.disabled} disabled, ${summary.skipped} skipped, ${summary.failed} failed.`
        : `Scheduled wipes: ${summary.created} created, ${summary.alreadyConfigured} already active, ${summary.disabledExisting} existing but disabled, ${summary.skipped} skipped, ${summary.failed} failed.`;

/**
 * A reduced post-clear count proves progress, but a failed cleaner cannot be
 * reported as a complete clear even if the fresh scan found zero.
 */
export function reconcileCleanupClear(
    before: number,
    after: number,
    backendSucceeded: boolean,
): CleanupClearReconciliation {
    const removed = Math.max(0, before - after);
    if (!backendSucceeded) return { result: 'failed', removed };
    if (after === 0) return { result: 'cleared', removed };
    if (removed > 0) return { result: 'reduced', removed };
    return { result: 'unchanged', removed };
}

export function verifyNtfsJournalReset(
    receipt: { verifiedReset?: boolean; journals?: Array<{ drive?: string; beforeJournalId?: string; afterJournalId?: string }> } | undefined,
    scan: { journals?: Array<{ drive?: string; journalId?: string }> } | undefined,
): boolean {
    if (receipt?.verifiedReset !== true || !Array.isArray(receipt.journals) || !receipt.journals.length || !Array.isArray(scan?.journals)) return false;
    return receipt.journals.every(journal => {
        const before = journal.beforeJournalId?.toLowerCase();
        const after = journal.afterJournalId?.toLowerCase();
        const current = scan.journals!.find(row => row.drive?.toLowerCase() === journal.drive?.toLowerCase())?.journalId?.toLowerCase();
        return !!before && !!after && before !== after && current === after;
    });
}

// Sentinel "account" for the combined All-Users view (#7).
export const ALL_USERS_KEY = '__all__';
function matchesCleanupAccountInventory(expected: readonly string[], reported: readonly string[], ready: boolean): boolean {
    const expectedKeys = expected.map(value => value.toLowerCase());
    const reportedKeys = reported.map(value => value.toLowerCase());
    const expectedSet = new Set(expectedKeys);
    const reportedSet = new Set(reportedKeys);
    return ready && expectedKeys.length > 0 && expectedKeys.every(Boolean) && reportedKeys.every(Boolean) &&
        expectedSet.size === expectedKeys.length && reportedSet.size === reportedKeys.length &&
        expectedKeys.length === reportedKeys.length && expectedKeys.every(key => reportedSet.has(key));
}
// Must match the allowlist implemented by Get-CleanupSummaryAllUsers and
// Invoke-CleanupClearAllUsers in privacy/cleanup.ps1. Current-user cards use
// their dedicated Get-*/Clear-* handlers and are not limited by this set.
export const MULTI_USER_CLEANUP_IDS = new Set<string>([
    'rdpHistory', 'recentFiles', 'jumpLists', 'psHistory',
    'browserFootprints', 'shellBags', 'execCache', 'netDrives',
    'ntUserTraces', 'notepadState', 'walFiles', 'crashDumps', 'recallDb',
    'webCache', 'thumbnailDb', 'notificationDb', 'activitiesTimeline',
    'rdpBitmapCache', 'developerCaches',
]);
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
    _cleanupCache[id] = retainCleanupVerification(data, _cleanupCache[id]);
    _cacheSubscriber?.({ ..._cleanupCache });
}

interface CleanupScanOptions {
    schedulesEnabled: boolean;
    entitlementsReady: boolean;
    migrationEnabled: boolean;
    auxiliaryStateEnabled?: boolean;
}

export function useCleanupScan({ schedulesEnabled, entitlementsReady, migrationEnabled, auxiliaryStateEnabled = true }: CleanupScanOptions) {
    const requestConfirm = useAppConfirm();
    const backend = useBackend(true);
    const { data: settings } = useSettingsQuery();
    const { mutate: persistSettings } = usePatchSettings();
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
        setIndividualAutoEraseSchedule,
        setMultiUserAutoEraseSchedule,
        removeMultiUserAutoEraseSchedule,
        setManagedAutoEraseSchedule,
        removeAutoEraseSchedule,
        removeManagedAutoEraseSchedule,
        getAutoEraseSchedules,
        invokeAutoEraseMigration,
        getUserProfiles,
        invokeCleanupClearAllUsers,
        getCleanupSummaryAllUsers,
        error,
    } = backend;

    // User exclusions may add protection; backend-protected defaults stay fixed.
    const [clearAllExcludes, setClearAllExcludesRaw] = useState<Set<string>>(
        () => new Set(DEFAULT_BULK_CLEAR_EXCLUDES)
    );
    const hasChangedExclusions = useRef(false);

    useEffect(() => {
        if (hasChangedExclusions.current || !settings) return;
        setClearAllExcludesRaw(
            new Set([...DEFAULT_BULK_CLEAR_EXCLUDES, ...(settings.app.bulkClearExcludes ?? [])]),
        );
    }, [settings]);

    useEffect(() => {
        if (!hasChangedExclusions.current) return;
        persistSettings(
            { app: { bulkClearExcludes: Array.from(clearAllExcludes) } },
            { onError: () => showError("Couldn't save bulk-clear exclusions.") },
        );
    }, [clearAllExcludes, persistSettings]);

    const setClearAllExcludes: React.Dispatch<React.SetStateAction<Set<string>>> = (update) => {
        hasChangedExclusions.current = true;
        setClearAllExcludesRaw((previous) => {
            return new Set([...DEFAULT_BULK_CLEAR_EXCLUDES, ...(typeof update === "function" ? update(previous) : update)]);
        });
    };

    // ── Multi-user viewer state ────────────────────────────────────────
    // The panel views exactly ONE user at a time. `availableUsers` lists every
    // real account; `currentUser` is the logged-in operator (the default
    // view); `selectedUser` is whose traces the grid currently shows. Only
    // admins may switch to another account — the backend independently
    // enforces this via Test-IsAdmin, so a non-admin is pinned to themselves.
    // When a non-current user is selected, scopeAware cards are populated from
    // Get-CleanupSummaryAllUsers scanned for that single user.
    const [availableUsers, setAvailableUsers] = useState<Array<{ name: string; sid?: string; displayName?: string; path: string; isCurrent?: boolean }>>([]);
    const [accountInventoryState, setAccountInventoryState] = useState<"loading" | "ready" | "error">("loading");
    const [currentUser, setCurrentUser] = useState<string>('');
    const [isAdminUser, setIsAdminUser] = useState<boolean>(false);
    const [selectedUser, setSelectedUser] = useState<string>('');
    const selectedUserRef = useRef(selectedUser);
    selectedUserRef.current = selectedUser;
    const [otherUserDataMap, setOtherUserDataMap] = useState<Record<string, CardData>>({});
    const [otherUserLoading, setOtherUserLoading] = useState(false);
    const [otherUserClearing, setOtherUserClearing] = useState(false);
    const profileReadGate = useRef(createCleanupProfileReadGate());
    const scheduleMutationBusy = useRef(false);
    const otherUserClearBusy = useRef(false);

    // ── Combined "All Users" view (#7) ──────────────────────────────────
    // Sums every account's per-category counts into one grid; the detail
    // modal breaks the total back down per user (e.g. alice 200 + bob 300 → 500).
    const [allUsersRaw, setAllUsersRaw] = useState<Array<{
        username: string;
        displayName: string;
        categories: Record<string, { count: number; items: string[]; raw?: unknown; error?: string }>;
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
    // display. Internal selection uses the Windows profile SID when available.
    const selectedDisplay = isViewingAllUsers
        ? 'All users'
        : (() => { const profile = availableUsers.find(u => cleanupProfileKey(u) === (selectedUser || currentUser)); return profile?.displayName || profile?.name || ''; })();

    // Scan one non-current user's scope-aware traces and project them onto the
    // card grid. One backend call returns every category for that user.
    const loadUserView = async (username: string, categoryIds?: string[]) => {
        const ticket = profileReadGate.current.begin();
        setOtherUserLoading(true);
        try {
            const res = await getCleanupSummaryAllUsers(categoryIds ?? [...MULTI_USER_CLEANUP_IDS, ...OTHER_ACCOUNT_READ_ONLY_IDS], [username]);
            if (!profileReadGate.current.isCurrent(ticket)) return false;
            if (res.success && res.data?.users?.length === 1 && !(res.data as { error?: boolean }).error &&
                ((/^S-1-5-21-/i.test(username) && res.data.users[0].sid?.toLowerCase() === username.toLowerCase()) ||
                 (!/^S-1-5-21-/i.test(username) && res.data.users[0].username?.toLowerCase() === username.toLowerCase()))) {
                const u = res.data.users[0];
                const byId: Record<string, { count: number; items: string[]; raw?: unknown; error?: string }> = {};
                for (const c of u.categories) {
                    byId[c.id] = readCleanupUserSummary(c);
                }
                const next: Record<string, CardData> = {};
                const requestedIds = categoryIds?.length ? new Set(categoryIds) : null;
                for (const cat of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]) {
                    if (!cat.scopeAware || (!MULTI_USER_CLEANUP_IDS.has(cat.id) && !OTHER_ACCOUNT_READ_ONLY_IDS.has(cat.id))) continue;
                    if (requestedIds && !requestedIds.has(cat.id)) continue;
                    const d = byId[cat.id];
                    next[cat.id] = {
                        count: d?.count ?? -1,
                        items: d?.items ?? [],
                        loading: false,
                        clearing: false,
                        raw: d?.raw,
                        error: d?.error ?? (d ? undefined : 'This category was not scanned.'),
                    };
                }
                const merged = categoryIds?.length
                    ? { ...(_otherUserCache[username] ?? {}), ...next }
                    : next;
                _otherUserCache[username] = merged;
                setOtherUserDataMap(merged);
                return categoryIds?.length === 1 && next[categoryIds[0]]?.count === 0 && !next[categoryIds[0]]?.error;
            } else {
                const message = res.error || `Failed to load traces for ${username}.`;
                showError(message);
                setOtherUserDataMap({});
                return false;
            }
        } catch (e) {
            if (!profileReadGate.current.isCurrent(ticket)) return false;
            showError(`Failed to load traces for ${username}: ${e}`);
            setOtherUserDataMap({});
            return false;
        } finally {
            if (profileReadGate.current.isCurrent(ticket)) setOtherUserLoading(false);
        }
    };

    // Load EVERY account's scope-aware traces, kept per-user, so the grid can
    // sum them and the detail modal can break the total back down (#7).
    const loadAllUsersView = async (categoryId?: string) => {
        const ticket = profileReadGate.current.begin();
        setOtherUserLoading(true);
        try {
            const requested = categoryId ? [categoryId] : [...MULTI_USER_CLEANUP_IDS, ...OTHER_ACCOUNT_READ_ONLY_IDS];
            const res = await getCleanupSummaryAllUsers(requested, []);
            if (!profileReadGate.current.isCurrent(ticket)) return false;
            if (res.success && res.data?.users?.length && !(res.data as { error?: boolean }).error) {
                // A one-category verification must not erase other captured
                // categories. Preserve them only for the same returned account;
                // profiles no longer in the response are not carried forward.
                setAllUsersRaw(previous => res.data!.users.map(u => {
                    const identity = u.sid || u.username;
                    const prior = categoryId && identity
                        ? previous.find(item => item.username.toLowerCase() === identity.toLowerCase())
                        : undefined;
                    const categories = prior ? { ...prior.categories } : {};
                    if (categoryId) delete categories[categoryId];
                    for (const c of u.categories ?? []) categories[c.id] = readCleanupUserSummary(c);
                    return {
                        username: identity,
                        displayName: availableUsers.find(a => cleanupProfileKey(a).toLowerCase() === identity.toLowerCase())?.displayName ?? u.username,
                        categories,
                    };
                }));
                const accountsMatch = matchesCleanupAccountInventory(
                    availableUsers.map(cleanupProfileKey), res.data.users.map(user => user.sid || user.username), accountInventoryState === 'ready');
                return accountsMatch && res.data.users.every(user => requested.every(id => {
                    const category = user.categories?.find(item => item.id === id);
                    return !!category && readCleanupUserSummary(category).count === 0;
                }));
            } else {
                showError(res.error || 'Failed to load combined user traces.');
                setAllUsersRaw([]);
                return false;
            }
        } catch (e) {
            if (!profileReadGate.current.isCurrent(ticket)) return false;
            showError(`Failed to load combined user traces: ${e}`);
            setAllUsersRaw([]);
            return false;
        } finally {
            if (profileReadGate.current.isCurrent(ticket)) setOtherUserLoading(false);
        }
    };

    // Switch the viewed user. Current user restores the normal per-category
    // grid. Another user only changes selection; scans are started explicitly
    // from that user's card/rescan controls so they target that profile only.
    const handleSwitchUser = (username: string) => {
        if (scheduleMutationBusy.current || otherUserClearBusy.current || otherUserLoading) return;
        profileReadGate.current.begin();
        setOtherUserLoading(false);
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
        // A combined total is only complete when the scan covers the same
        // visible accounts as the account picker. If inventory changed or a
        // response omitted an account, keep returned observations but do not
        // present their sum as the total for all users.
        const accountsMatch = matchesCleanupAccountInventory(
            availableUsers.map(cleanupProfileKey), allUsersRaw.map(user => user.username), accountInventoryState === 'ready');
        for (const cat of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES]) {
            if (!cat.scopeAware || (!MULTI_USER_CLEANUP_IDS.has(cat.id) && !OTHER_ACCOUNT_READ_ONLY_IDS.has(cat.id))) continue;
            let total = 0;
            const breakdown: string[] = [];
            let incomplete = !accountsMatch;
            for (const u of allUsersRaw) {
                const d = u.categories[cat.id];
                const c = d?.count ?? -1;
                if (c < 0 || d?.error) incomplete = true;
                if (c > 0) total += c;
                if (c > 0) breakdown.push(`${u.displayName} — ${c}`);
            }
            map[cat.id] = { count: incomplete ? -1 : total, items: breakdown, loading: false, clearing: false,
                error: incomplete && allUsersRaw.length > 0
                    ? !accountsMatch ? 'Account inventory changed or a profile was missing. Refresh accounts and scan again.' : 'One or more user scans were incomplete.'
                    : undefined };
        }
        return map;
    }, [allUsersRaw, availableUsers, accountInventoryState]);

    // ── Auto-erase scheduler state ───────────────────────────────────
    // schedulesById: categoryId → intervalMinutes for cards with an
    // active Windows Scheduled Task. Pre-populated on panel mount.
    // The set of supported categoryIds is hardcoded in
    // cleanupCategories.ts (SUPPORTED_AUTOERASE_IDS) so the clock icon
    // appears instantly — no async roundtrip just to decide visibility.
    // Render the last verified snapshot immediately. Task Scheduler remains
    // authoritative and replaces this cache silently after panel startup.
    const [schedulesById, setSchedulesById] = useState<Record<string, number>>(readScheduleCache);
    const [scheduleRecordsById, setScheduleRecordsById] = useState<NonNullable<ReturnType<typeof readCleanupScheduleSnapshot>>["records"]>({});
    const [scheduleRows, setScheduleRows] = useState<CleanupScheduleRow[] | null>(null);
    const [disabledSchedulesById, setDisabledSchedulesById] = useState<Record<string, { intervalMinutes: number | null; managedByAutoSet: boolean; scope: CleanupScheduleScope }>>({});
    const [scheduleState, setScheduleState] = useState<"checking" | "verified" | "unavailable">("checking");
    const [autoSetScheduleIds, setAutoSetScheduleIds] = useState<Set<string>>(new Set());
    const [scheduleBusyId, setScheduleBusyId] = useState<string | null>(null);
    const [autoSetSchedulesBusy, setAutoSetSchedulesBusy] = useState(false);
    const [migrationIncomplete, setMigrationIncomplete] = useState(false);
    const migrationGate = useRef(createCleanupScheduleMigrationGate());
    const scheduleReadGate = useRef(createCleanupScheduleReadGate());
    const visibleScheduleState = scheduleState;
    const selectedOwnerSid = !isViewingCurrentUser && !isViewingAllUsers && isAdminUser &&
        /^S-1-5-21-\d+(?:-\d+)+$/i.test(selectedUser) &&
        availableUsers.some(profile => profile.sid?.toLowerCase() === selectedUser.toLowerCase()) ? selectedUser : null;
    const otherScheduleSnapshot = useMemo(() => selectedOwnerSid && scheduleRows
        ? readCleanupScheduleSnapshot(scheduleRows, CLEANUP_SCHEDULE_IDS, { ownerSid: selectedOwnerSid }) : null,
        [selectedOwnerSid, scheduleRows]);
    const otherScheduleState = scheduleState === 'verified' && !otherScheduleSnapshot ? 'unavailable' : scheduleState;

    const refreshSchedules = async (verifyOwnerSid?: string) => {
        const ticket = scheduleReadGate.current.begin();
        setScheduleState("checking");
        if (entitlementsReady && schedulesEnabled && migrationEnabled) {
            let migrationError = "Scheduled cleanup upgrade could not be verified.";
            const migrated = await migrationGate.current.ensure(async () => {
                const result = await invokeAutoEraseMigration();
                if (!isVerifiedCleanupMigration(result)) {
                    migrationError = result.error || migrationError;
                    return false;
                }
                invalidateDiskCleanupScheduleStatus();
                return true;
            });
            if (!scheduleReadGate.current.isCurrent(ticket)) return null;
            setMigrationIncomplete(!migrated);
            if (!migrated) showError(migrationError);
        }
        let res;
        try { res = await loadDiskCleanupSchedules(getAutoEraseSchedules, true); }
        catch { if (scheduleReadGate.current.isCurrent(ticket)) { setScheduleRows(null); setScheduleState("unavailable"); } return null; }
        if (!scheduleReadGate.current.isCurrent(ticket)) return null;
        if (isCleanupCommandSuccessful(res) && Array.isArray(res.data?.schedules)) {
            const snapshot = readCleanupScheduleSnapshot(res.data.schedules, CLEANUP_SCHEDULE_IDS);
            if (!snapshot) { setScheduleRows(null); setScheduleState("unavailable"); return null; }
            setScheduleRows(res.data.schedules);
            setSchedulesById(snapshot.schedules);
            setScheduleRecordsById(snapshot.records);
            setDisabledSchedulesById(snapshot.disabledSchedules);
            writeScheduleCache(snapshot.schedules);
            setAutoSetScheduleIds(snapshot.managedIds);
            setScheduleState("verified");
            return { ...snapshot, selectedSnapshot: verifyOwnerSid ? readCleanupScheduleSnapshot(res.data.schedules, CLEANUP_SCHEDULE_IDS, { ownerSid: verifyOwnerSid }) : null };
        }
        setScheduleRows(null);
        setScheduleState("unavailable");
        return null;
    };

    const refreshAccountInventory = async () => {
        setAccountInventoryState("loading");
        const result = await readCleanupProfileInventory(getUserProfiles);
        if (!result.ok) {
            setAccountInventoryState("error");
            return;
        }
        const data = result.data;
        const realProfiles = (data.profiles ?? []).filter(isVisibleCleanupProfile);
        const current = resolveCurrentCleanupProfile(realProfiles, data.currentUser, data.currentSid);
        setAvailableUsers(realProfiles);
        setCurrentUser(current);
        setIsAdminUser(!!data.isAdmin);
        setSelectedUser((prev) => prev || current);
        setAccountInventoryState("ready");
    };

    useEffect(() => {
        if (!auxiliaryStateEnabled) return;
        void refreshAccountInventory();
        void refreshSchedules();
        // Stable refs from useBackend's useMemo — safe to omit from deps.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [auxiliaryStateEnabled]);

    useEffect(() => {
        if (!entitlementsReady || !schedulesEnabled || !migrationEnabled) return;
        // KT: migration mutates scheduled tasks and is paid. Running it before
        // entitlement and Pro-install resolution could asynchronously cover
        // an open scheduler with LicenseGate or the Pro installer.
        void refreshSchedules();
        // Stable refs from useBackend's useMemo — safe to omit from deps.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [entitlementsReady, schedulesEnabled, migrationEnabled]);

    const handleSetSchedule = async (categoryId: string, minutes: number, runAsSystem: boolean) => {
        const category = SCHEDULABLE_CATEGORIES.find(candidate => candidate.id === categoryId);
        const ownerSid = !isViewingCurrentUser && category?.scopeAware && !runAsSystem ? selectedOwnerSid : null;
        if (scheduleMutationBusy.current || otherUserClearBusy.current || autoSetSchedulesBusy || !category?.schedulable || !schedulesEnabled || visibleScheduleState !== "verified" ||
            (!isViewingCurrentUser && category.scopeAware && (!ownerSid || otherScheduleState !== 'verified'))) return false;
        const allowDefaultExcluded = isDefaultExcludedCleanup(categoryId);
        scheduleMutationBusy.current = true;
        setScheduleBusyId(categoryId);
        try {
            if (!await requestCleanupManualAuthorization(category, 'schedule', requestConfirm, minutes, ownerSid ? `${selectedDisplay} (${ownerSid})` : undefined)) return false;
            if (ownerSid && selectedUserRef.current !== ownerSid) return false;
            const schedulerId = getSchedulerCategoryId(categoryId);
            const res = ownerSid
                ? await setMultiUserAutoEraseSchedule(schedulerId, minutes, [ownerSid], false, allowDefaultExcluded)
                : await setIndividualAutoEraseSchedule(schedulerId, minutes, runAsSystem, allowDefaultExcluded);
            if (!isCleanupCommandSuccessful(res)) {
                showError(res.error || `Failed to schedule auto-clean for ${category.label}`);
                return false;
            }
            invalidateDiskCleanupScheduleStatus();
            const verified = await refreshSchedules(ownerSid ?? undefined);
            const payload = res.data as { intervalMinutes?: number; total?: number; results?: Array<{ intervalMinutes?: number; status?: string }> } | undefined;
            const registeredMinutes = ownerSid ? (payload?.total === 1 && payload.results?.length === 1 && payload.results[0].status === 'enabled' ? payload.results[0].intervalMinutes : undefined) : payload?.intervalMinutes;
            const record = ownerSid ? verified?.selectedSnapshot?.records[categoryId] : verified?.records[categoryId];
            if (!isVerifiedIndividualSchedule(record, minutes, registeredMinutes, ownerSid ? 'other-user' : runAsSystem ? 'system' : 'current-user', ownerSid ?? undefined)) {
                showError(`${category.label}: the enabled schedule could not be confirmed. Check Task Scheduler and retry.`);
                return false;
            }
            showSuccess(`Auto-clean scheduled every ${minutes} min for ${category.label}${ownerSid ? ` — ${selectedDisplay}` : ''}`);
            return true;
        } finally {
            scheduleMutationBusy.current = false;
            setScheduleBusyId(null);
        }
    };

    const handleClearSchedule = async (categoryId: string) => {
        const category = SCHEDULABLE_CATEGORIES.find(candidate => candidate.id === categoryId);
        const ownerSid = !isViewingCurrentUser && category?.scopeAware && !category.schedulerRunAsSystem ? selectedOwnerSid : null;
        if (scheduleMutationBusy.current || otherUserClearBusy.current || autoSetSchedulesBusy || !category || (!isViewingCurrentUser && category.scopeAware && (!ownerSid || otherScheduleState !== 'verified'))) return false;
        scheduleMutationBusy.current = true;
        setScheduleBusyId(categoryId);
        try {
            const schedulerId = getSchedulerCategoryId(categoryId);
            const res = ownerSid ? await removeMultiUserAutoEraseSchedule(schedulerId, [ownerSid]) : await removeAutoEraseSchedule(schedulerId);
            if (!isCleanupCommandSuccessful(res)) {
                showError(res.error || `Failed to turn off auto-clean for ${categoryId}`);
                return false;
            }
            invalidateDiskCleanupScheduleStatus();
            const verified = await refreshSchedules(ownerSid ?? undefined);
            const snapshot = ownerSid ? verified?.selectedSnapshot : verified;
            if (!snapshot || snapshot.records[categoryId] !== undefined) {
                showError(`${categoryId}: schedule removal could not be confirmed. Check Task Scheduler and retry.`);
                return false;
            }
            showSuccess(`Auto-clean turned off for ${categoryId}${ownerSid ? ` — ${selectedDisplay}` : ''}`);
            return true;
        } finally {
            scheduleMutationBusy.current = false;
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

    const clearerMap: Record<string, ((allowDefaultExcluded?: boolean) => Promise<any>) | undefined> = {
        shellBags: clearShellBags,
        usbHistory: clearUsbHistory,
        dnsCache: flushDnsCache,
        execCache: clearExecutionCache,
        wlanProfiles: (allowDefaultExcluded) => removeWlanProfile('', allowDefaultExcluded),
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
        browserFootprints: (allowDefaultExcluded) => clearBrowserFootprints(undefined, undefined, allowDefaultExcluded),
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
        unallocatedErase: () => invokeUnallocatedSpaceErase(),
        // Drive optimization is a single OsRepairCard action. Windows chooses
        // defrag or ReTrim per volume, so no SSD-only cleanup action is
        // dispatched through this map.
    };

    // Keep the card dispatcher in lockstep with cleanupCategories.ts. Most
    // original categories are listed explicitly above because a few need an
    // argument adapter (notably WLAN), but newly-added no-argument viewer and
    // clearer methods can be resolved safely from their declared backend keys.
    // This also prevents a backend-complete card from silently becoming a
    // no-op when a category is added without duplicating it in both maps.
    const backendNoArg = (methodName: string): ((allowDefaultExcluded?: boolean) => Promise<any>) | undefined => {
        const candidate = (backend as unknown as Record<string, unknown>)[methodName];
        return typeof candidate === 'function'
            ? (allowDefaultExcluded) => (candidate as (allowDefaultExcluded?: boolean) => Promise<any>)(allowDefaultExcluded)
            : undefined;
    };
    for (const cat of [...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES, ...VIEW_ONLY_CATEGORIES, ...ACTION_CATEGORIES]) {
        if (!getterMap[cat.id] && cat.getDataKey) getterMap[cat.id] = backendNoArg(cat.getDataKey);
        if (!clearerMap[cat.id] && cat.clearDataKey) clearerMap[cat.id] = backendNoArg(cat.clearDataKey);
    }

    // Load a single category — writes to module cache so results survive unmount
    const loadSingleCategory = async (cat: CleanupCategory): Promise<CardData | undefined> => {
        const getter = getterMap[cat.id];
        if (!getter) return;
        updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: -1, items: [] }), loading: true, clearing: false });
        try {
            const res = await getter();
            const data = readCleanupScanResult(cat, res);
            updateCacheEntry(cat.id, data);
            return data;
        } catch (e) {
            const data: CardData = { count: -1, items: [], loading: false, clearing: false, error: String(e) };
            updateCacheEntry(cat.id, data);
            return data;
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
            await runCleanupWorkers(cats, async cat => { await loadSingleCategory(cat); }, concurrency);
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
    type ClearOutcome = {
        result: CleanupClearResult;
        before: number;
        after: number;
        removed: number;
        error?: string;
    };
    const clearAndReconcile = async (cat: CleanupCategory, allowDefaultExcluded = false): Promise<ClearOutcome> => {
        const before = Math.max(0, _cleanupCache[cat.id]?.count ?? cardDataMap[cat.id]?.count ?? 0);
        const clearer = clearerMap[cat.id];
        if (!clearer) return { result: 'failed', before, after: before, removed: 0 };
        const recordVerification = (outcome: ClearOutcome): ClearOutcome => {
            const current = _cleanupCache[cat.id];
            if (current) updateCacheEntry(cat.id, {
                ...current, loading: false, clearing: false,
                verification: createCleanupVerification(outcome.result),
            });
            return outcome;
        };

        updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: before, items: [] }), clearing: true, loading: false });
        let backendSucceeded = false;
        let error: string | undefined;
        try {
            const res = await clearer(allowDefaultExcluded);
            const payload = res.data as { ok?: boolean; error?: boolean; message?: string; stdout?: string;
                verifiedReset?: boolean; journals?: Array<{ drive?: string; beforeJournalId?: string; afterJournalId?: string }> } | undefined;
            backendSucceeded = payload?.ok === true && isCleanupCommandSuccessful(res);
            error = res.error || (payload?.error || payload?.ok === false ? payload.message || payload.stdout : undefined);
            const bootScheduled = cat.id === 'amcache' && backendSucceeded
                ? Number(payload?.stdout?.match(/\bbootScheduled=(\d+)/i)?.[1] ?? 0)
                : 0;
            const fresh = await loadSingleCategory(cat);
            if (!fresh || fresh.error || fresh.count < 0) {
                return recordVerification({ result: 'failed', before, after: before, removed: 0, error: fresh?.error || 'A complete verification scan was not available.' });
            }
            const after = fresh.count;
            if (bootScheduled > 0) return recordVerification({ result: 'deferred', before, after, removed: 0 });
            if (cat.id === 'ntfsJournals') {
                const resetVerified = backendSucceeded && verifyNtfsJournalReset(payload, fresh.raw);
                return recordVerification({ result: resetVerified ? 'reset' : 'failed', before, after, removed: 0,
                    error: resetVerified ? undefined : 'Journal reset could not be verified by identity.' });
            }
            return recordVerification({ ...reconcileCleanupClear(before, after, backendSucceeded), before, after, error });
        } catch (cause) {
            const fresh = await loadSingleCategory(cat);
            if (!fresh || fresh.error || fresh.count < 0) {
                return recordVerification({ result: 'failed', before, after: before, removed: 0, error: fresh?.error || 'A complete verification scan was not available.' });
            }
            const after = fresh.count;
            error = String(cause);
            return recordVerification({ ...reconcileCleanupClear(before, after, false), before, after, error });
        }
    };

    /**
     * Applies only defaults where no task exists. Existing entries are treated
     * as intentional user choices and are never overwritten by this shortcut.
     */
    const handleAutoSetSchedules = async (): Promise<AutoSetScheduleSummary> => {
        let firstFailure: string | undefined;
        const summary: AutoSetScheduleSummary = {
            created: 0,
            alreadyConfigured: 0,
            disabledExisting: 0,
            disabled: 0,
            skipped: 0,
            failed: 0,
        };
        if (scheduleMutationBusy.current || otherUserClearBusy.current || !canAutoSetCleanupSchedules(schedulesEnabled, visibleScheduleState, migrationIncomplete, autoSetScheduleIds.size > 0)) {
            if (migrationIncomplete) showError("Resolve the scheduled cleanup upgrade before using Auto-set.");
            summary.skipped = SCHEDULABLE_CATEGORIES.length;
            return summary;
        }

        scheduleMutationBusy.current = true;
        setAutoSetSchedulesBusy(true);
        const createdIds: Array<{ id: string; interval: number; scope: CleanupScheduleScope }> = [];
        const disabledIds: string[] = [];
        const noOpRemovalIds: string[] = [];
        const existingActiveIds: string[] = [];
        const existingDisabledIds: string[] = [];
        let verifyTrimCreation = false;
        try {
            const managedIds = Array.from(autoSetScheduleIds);
            if (managedIds.length > 0) {
                for (const categoryId of managedIds) {
                    if (requiresCleanupTaskRepair(scheduleRecordsById[categoryId])) {
                        summary.failed++;
                        firstFailure ??= `Review the ${categoryId} task in Windows Task Scheduler before changing it.`;
                        continue;
                    }
                    try {
                        // A distinct backend command fails closed against an older
                        // sidecar and removes only tasks bearing the managed marker.
                        const res = await removeManagedAutoEraseSchedule(getSchedulerCategoryId(categoryId));
                        if (isCleanupCommandSuccessful(res) && Array.isArray(res.data?.removed)) {
                            if (res.data.removed.length > 0) { summary.disabled++; disabledIds.push(categoryId); }
                            else { summary.skipped++; noOpRemovalIds.push(categoryId); }
                        } else {
                            summary.failed++;
                            firstFailure ??= res.error || `Failed to turn off ${categoryId}.`;
                        }
                    } catch {
                        summary.failed++;
                        firstFailure ??= `Failed to turn off ${categoryId}.`;
                    }
                }
            } else {
                const eligibleCategories = SCHEDULABLE_CATEGORIES.filter(category =>
                    category.schedulable && category.clearDataKey && !isAutomaticCleanupExcluded(category.id, clearAllExcludes));
                if (!await requestConfirm(getCleanupAutoScheduleConfirmation(eligibleCategories))) {
                    summary.skipped = SCHEDULABLE_CATEGORIES.length;
                    return summary;
                }
                for (const category of SCHEDULABLE_CATEGORIES) {
                    if (!category.schedulable || !category.clearDataKey || isAutomaticCleanupExcluded(category.id, clearAllExcludes)) {
                        summary.skipped++;
                        continue;
                    }
                    if (requiresCleanupTaskRepair(scheduleRecordsById[category.id])) {
                        summary.failed++;
                        firstFailure ??= `Review the ${category.label} task in Windows Task Scheduler before changing it.`;
                        continue;
                    }
                    const interval = getAutoScheduleInterval(
                        category,
                        cardDataMap[category.id]?.count,
                    );
                    try {
                        const res = await setManagedAutoEraseSchedule(
                            getSchedulerCategoryId(category.id),
                            interval,
                            !!category.schedulerRunAsSystem,
                        );
                        if (isCleanupCommandSuccessful(res) && res.data?.status === "alreadyConfigured") {
                            if (res.data.taskState === 'Disabled') { summary.disabledExisting++; existingDisabledIds.push(category.id); }
                            else if (['Ready', 'Running', 'Queued'].includes(res.data.taskState ?? '')) { summary.alreadyConfigured++; existingActiveIds.push(category.id); }
                            else { summary.failed++; firstFailure ??= `Existing schedule status for ${category.label} is unknown.`; }
                        } else if (isCleanupCommandSuccessful(res)) {
                            summary.created++;
                            createdIds.push({ id: category.id, interval, scope: category.schedulerRunAsSystem ? 'system' : 'current-user' });
                        }
                        else {
                            summary.failed++;
                            firstFailure ??= res.error || `Failed to schedule ${category.label}.`;
                        }
                    } catch {
                        summary.failed++;
                        firstFailure ??= `Failed to schedule ${category.label}.`;
                    }
                }
                // Schedule a daily SSD/NVMe ReTrim alongside the privacy
                // wipes.  This is a Windows Scheduled Task running as SYSTEM,
                // so it continues after navigating away from Cleanup or
                // closing WinCommander.  The embedded action skips HDDs.
                try {
                    if (requiresCleanupTaskRepair(scheduleRecordsById[AUTO_SCHEDULE_TRIM_ID])) throw new Error('Review the SSD maintenance task in Windows Task Scheduler before changing it.');
                    const trim = await setManagedAutoEraseSchedule(
                        AUTO_SCHEDULE_TRIM_ID,
                        AUTO_SCHEDULE_TRIM_INTERVAL_MINUTES,
                        true,
                    );
                    if (!isCleanupCommandSuccessful(trim)) {
                        summary.failed++;
                        firstFailure ??= trim.error || "Failed to schedule automatic SSD trim.";
                    } else if (trim.data?.status === 'alreadyConfigured') {
                        if (trim.data.taskState === 'Disabled') { summary.disabledExisting++; existingDisabledIds.push(AUTO_SCHEDULE_TRIM_ID); }
                        else if (['Ready', 'Running', 'Queued'].includes(trim.data.taskState ?? '')) { summary.alreadyConfigured++; existingActiveIds.push(AUTO_SCHEDULE_TRIM_ID); }
                        else { summary.failed++; firstFailure ??= 'Existing SSD maintenance schedule status is unknown.'; }
                    } else verifyTrimCreation = true;
                } catch (error) {
                    summary.failed++;
                    firstFailure ??= error instanceof Error ? error.message : "Failed to schedule automatic SSD trim.";
                }
            }
        } finally {
            // Always re-read the source of truth so every clock icon reflects
            // both successful tasks and any pre-existing/manual configuration.
            invalidateDiskCleanupScheduleStatus();
            let readback: Awaited<ReturnType<typeof refreshSchedules>> = null;
            try { readback = await refreshSchedules(); } catch {}
            for (const { id, interval, scope } of createdIds) {
                if (!readback || !isVerifiedManagedSchedule(readback.records[id], interval, scope)) {
                    summary.created--; summary.failed++;
                    firstFailure ??= 'One or more created schedules could not be verified.';
                }
            }
            for (const id of existingActiveIds) {
                if (!readback?.records[id]?.enabled) {
                    summary.alreadyConfigured--; summary.failed++;
                    firstFailure ??= 'One or more existing schedules could not be verified as active.';
                }
            }
            for (const id of existingDisabledIds) {
                if (!readback?.records[id] || readback.records[id].enabled) {
                    summary.disabledExisting--; summary.failed++;
                    firstFailure ??= 'One or more existing schedules could not be verified as disabled.';
                }
            }
            for (const id of disabledIds) {
                if (!readback || readback.managedIds.has(id)) {
                    summary.disabled--; summary.failed++;
                    firstFailure ??= 'One or more removed schedules could not be verified.';
                }
            }
            for (const id of noOpRemovalIds) {
                if (!readback || readback.managedIds.has(id)) {
                    summary.skipped--; summary.failed++;
                    firstFailure ??= 'One or more schedules could not be confirmed as off.';
                }
            }
            if (verifyTrimCreation && (!readback || !isVerifiedManagedSchedule(readback.records[AUTO_SCHEDULE_TRIM_ID], AUTO_SCHEDULE_TRIM_INTERVAL_MINUTES, 'system'))) {
                summary.failed++;
                firstFailure ??= 'The SSD maintenance schedule could not be verified.';
            }
            scheduleMutationBusy.current = false;
            setAutoSetSchedulesBusy(false);
        }

        const message = `${formatAutoSetScheduleSummary(summary)}${firstFailure ? ` First error: ${firstFailure}` : ''}`;
        if (summary.failed > 0) showError(message);
        else showSuccess(message);
        return summary;
    };

    const handleCardClear = async (cat: CleanupCategory, onDriveWipe?: () => void) => {
        const allowDefaultExcluded = isDefaultExcludedCleanup(cat.id);
        if (!await requestCleanupManualAuthorization(cat, 'clear', requestConfirm)) return;
        if (cat.id === 'unallocatedErase') {
            onDriveWipe?.();
            return;
        }
        if (cat.confirmMessage && !allowDefaultExcluded) {
            const accepted = await requestConfirm({
                title: `Clear ${cat.label}?`,
                description: cat.confirmMessage,
                confirmLabel: "Clear",
            });
            if (!accepted) return;
        }
        // One-time actions have no scanner, so they need their own receipt.
        if (cat.actionOnly) {
            const clearer = clearerMap[cat.id];
            if (!clearer) return;
            updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: -1, items: [] }), clearing: true, loading: false });
            try {
                const res = await clearer();
                const receipt = res.data as { ok?: boolean; status?: string; stdout?: string } | undefined;
                if (res.success && receipt?.ok === true && receipt.status !== 'partial' && !/^fail:/im.test(receipt.stdout ?? '')) {
                    updateCacheEntry(cat.id, { count: -1, items: [], loading: false, clearing: false });
                    const message = cat.id === 'virtualMemory'
                        ? 'configured; shutdown or restart is required to finish.'
                        : receipt.status === 'started' ? 'started in background.' : 'completed.';
                    showSuccess(`${cat.label}: ${message}`);
                } else {
                    showError(res.error || receipt?.stdout || `${cat.label} was not completed.`);
                    updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: -1, items: [] }), clearing: false, loading: false });
                }
            } catch (e) {
                showError(`Failed to run ${cat.label}: ${e}`);
                updateCacheEntry(cat.id, { ...(_cleanupCache[cat.id] || { count: -1, items: [] }), clearing: false, loading: false });
            }
            return;
        }

        const outcome = await clearAndReconcile(cat, allowDefaultExcluded);
        if (outcome.result === 'cleared') {
            showSuccess(`${cat.label}: the fresh scan found no records.`);
        } else if (outcome.result === 'reduced') {
            showSuccess(`${cat.label}: the scan count fell from ${outcome.before} to ${outcome.after}.`);
        } else if (outcome.result === 'reset') {
            showSuccess(`${cat.label}: new journal identities were confirmed. Windows keeps active journals, so the card count may stay the same.`);
        } else if (outcome.result === 'deferred') {
            showSuccess(`${cat.label}: deletion is scheduled for restart. Scan again after restarting to verify it.`);
        } else if (outcome.result === 'unchanged') {
            showError(`${cat.label} still reports ${outcome.after} traces after cleanup.${outcome.error ? ` ${outcome.error}` : ''}`);
        } else {
            showError(outcome.error || `Failed to clear ${cat.label}.`);
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
            !isDefaultExcludedCleanup(cat.id) &&
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
        const results = new Map<string, CleanupClearReconciliation>();
        await runCleanupWorkers(withFindings, async cat => {
            results.set(cat.id, await clearAndReconcile(cat));
        });
        const cleared = withFindings.filter(cat => results.get(cat.id)?.result === 'cleared');
        const reduced = withFindings.filter(cat => results.get(cat.id)?.result === 'reduced');
        const reset = withFindings.filter(cat => results.get(cat.id)?.result === 'reset');
        const unchanged = withFindings.filter(cat => results.get(cat.id)?.result === 'unchanged');
        const deferred = withFindings.filter(cat => results.get(cat.id)?.result === 'deferred');
        const failed = withFindings.filter(cat => results.get(cat.id)?.result === 'failed');
        if (cleared.length || reduced.length || reset.length || deferred.length) {
            const summary = [
                cleared.length ? `${cleared.length} with no records on rescan` : '',
                reduced.length ? `${reduced.length} with lower scan counts` : '',
                reset.length ? `${reset.length} journals reset and verified by identity` : '',
                deferred.length ? `${deferred.length} scheduled for restart` : '',
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

    const handleClearAllCategories = async () =>
        handleClearCategories(orderedScanCategories, 'all cleanup findings', clearAllExcludes);

    // ── Other-users sub-section ─────────────────────────────────────────
    // Clears for the selected OTHER user (or all users) write into the
    // sub-section's own data map, so the main current-user grid is never
    // touched. `targets` undefined = all users.
    const runOtherUserClear = async (cat: CleanupCategory, targets: string[] | undefined, allowDefaultExcluded = false) => {
        if (isDefaultExcludedCleanup(cat.id) && !allowDefaultExcluded) return;
        const prev = otherUserDataMap[cat.id] || { count: -1, items: [] };
        const setCard = (data: CardData) => {
            setOtherUserDataMap(p => ({ ...p, [cat.id]: data }));
            // Keep the per-user cache in sync so switching away and back shows
            // the post-clear state, not stale pre-clear counts.
            if (selectedUser && _otherUserCache[selectedUser]) {
                _otherUserCache[selectedUser] = { ..._otherUserCache[selectedUser], [cat.id]: data };
            }
        };
        // Keep the count while clearing so the card footer + "Clearing..." show.
        setCard({ count: prev.count, items: prev.items ?? [], loading: false, clearing: true });
        try {
            const res = await invokeCleanupClearAllUsers([cat.id], targets, allowDefaultExcluded);
            if (res.success && !(res.data as { error?: boolean } | undefined)?.error) {
                const d = res.data;
                const verifiedEmpty = targets && targets.length
                    ? await loadUserView(targets[0], [cat.id])
                    : await loadAllUsersView(cat.id);
                if ((d?.partial ?? 0) > 0 || !verifiedEmpty) {
                    showError(`${cat.label}: cleanup was partial or its result could not be verified on a fresh scan.`);
                } else {
                    showSuccess(`${cat.label}: a fresh scan found no records for the selected users.`);
                }
            } else {
                showError(res.error || `Failed to clear ${cat.label}.`);
                setCard({ count: prev.count, items: prev.items ?? [], loading: false, clearing: false });
            }
        } catch (e) {
            showError(`Failed to clear ${cat.label}: ${e}`);
            setCard({ count: prev.count, items: prev.items ?? [], loading: false, clearing: false });
        }
    };

    // "Clear for {selected user}" in the sub-section.
    const handleOtherUserClear = async (cat: CleanupCategory) => {
        if (!selectedUser || !MULTI_USER_CLEANUP_IDS.has(cat.id) || otherUserClearBusy.current || scheduleMutationBusy.current) return;
        otherUserClearBusy.current = true;
        setOtherUserClearing(true);
        try {
            const accepted = await requestConfirm({
                title: `Clear ${cat.label}?`,
                description: `${isDefaultExcludedCleanup(cat.id) ? getCleanupManualWarning(cat) + " This card is excluded from bulk cleanup by default. " : ""}Clear ${cat.label} for ${selectedDisplay}? The selected profile will be re-scanned afterward. This cannot be undone.`,
                confirmLabel: "Clear profile",
            });
            if (!accepted || selectedUserRef.current !== selectedUser) return;
            await runOtherUserClear(cat, [selectedUser], isDefaultExcludedCleanup(cat.id));
        } finally {
            otherUserClearBusy.current = false;
            setOtherUserClearing(false);
        }
    };

    // "Clear for all users" in the sub-section (admin-only, scopeAware-only).
    const handleCardClearAllUsers = async (cat: CleanupCategory) => {
        if (!MULTI_USER_CLEANUP_IDS.has(cat.id) || otherUserClearBusy.current || scheduleMutationBusy.current) return;
        otherUserClearBusy.current = true;
        setOtherUserClearing(true);
        try {
            const accepted = await requestConfirm({
                title: `Clear ${cat.label} for all users?`,
                description: `${isDefaultExcludedCleanup(cat.id) ? getCleanupManualWarning(cat) + " This card is excluded from bulk cleanup by default. " : ""}Clear ${cat.label} for all user profiles the backend can access? The ${availableUsers.length} detected profiles shown here may not include every account; this action may affect profiles beyond the list. This cannot be undone.`,
                confirmLabel: "Clear all users",
            });
            if (!accepted) return;
            await runOtherUserClear(cat, undefined, isDefaultExcludedCleanup(cat.id));
        } finally {
            otherUserClearBusy.current = false;
            setOtherUserClearing(false);
        }
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
        handleClearAllCategories,
        clearAllExcludes,
        setClearAllExcludes,
        availableUsers,
        accountInventoryState,
        refreshAccountInventory,
        currentUser,
        isAdminUser,
        selectedUser,
        otherUserDataMap,
        otherUserLoading,
        otherUserClearing,
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
        otherScheduleSnapshot,
        otherScheduleState,
        selectedOwnerSid,
        disabledSchedulesById,
        scheduleRecordsById,
        scheduleState: visibleScheduleState,
        refreshSchedules,
        hasScheduledWipes: visibleScheduleState === "verified" && autoSetScheduleIds.size > 0,
        scheduleBusyId,
        autoSetSchedulesBusy,
        handleSetSchedule,
        handleClearSchedule,
        handleAutoSetSchedules,
    };
}

