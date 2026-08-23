import React, { createContext, useContext, useState, useCallback, useEffect, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useQueryClient } from '@tanstack/react-query';
import { useAuthMode } from './AuthModeContext';
import { DECOY_APP_SETTINGS, DECOY_INVENTORY } from '../lib/decoyFakeData';
import { settingsKeys } from '../hooks/queries/useSettingsQuery';
import useBackend, {
    SystemInfo,
    DriveSmartHealthResult,
    HardeningStatus,
    AppPrivacyCapabilitiesStatus,
    EncryptionStatus,
    MeshVPNStatus,
    DefenderStatus,
    UpdateStatus,
    BlocklistStatus,
    DNSStatus,
    executeBackendCommand
} from '../hooks/useBackend';
import type { AppSettings, SettingsPatch, AppInventorySnapshot } from '../types/settings';
import type { DependencyInfo } from '../hooks/useDependencies';
import { _getOperationHandlers } from './TaskStatusContext';
import { getDefaultModules } from '../types/modules';
import type { ModuleConfig } from '../types/modules';
import { getStartupStaggerStep } from '../lib/performancePolicy';
import { waitForSoftTimeout } from '../lib/softTimeout';
import { canRunStartupJob, type StartupEligibility } from '../lib/startupJobPolicy';
import { createStartupCoordinator, type StartupCoordinator, type StartupJob, type StartupJobResult } from '../services/startupCoordinator';
import { createStartupProbeStore } from '../services/startupProbeStore';
import { useLicenseQuery } from '../hooks/queries/useLicenseQuery';
import { createTauriStartupReporter } from '../events/startup';
import { reportStartupPhase } from '../hooks/startupTrace';

interface AppState {
    systemInfo: SystemInfo | null;
    meshInstalled: boolean | null;
    meshStatus: MeshVPNStatus | null;
    defenderStatus: DefenderStatus | null;
    updateStatus: UpdateStatus | null;
    networkBlocklistStatus: BlocklistStatus | null;
    networkDnsStatus: DNSStatus | null;
    encryptionStatus: EncryptionStatus | null;
    productivityStatus: { installed?: boolean; running: boolean; details: { server: boolean; input: boolean; active: boolean } } | null;

    // App Inventory — persisted snapshot from settings.json → current.apps.inventory
    appInventory: AppInventorySnapshot | null;

    // Unified Settings
    appSettings: AppSettings | null;
    /** Accepts a plain patch, or an updater `(latest) => patch` resolved at this
     *  write's turn in the serialized queue. Use the updater form when the patch
     *  is derived from shared array/collection state (e.g. lockedPanelIds,
     *  borrowedHidden) — the backend replaces arrays wholesale rather than
     *  deep-merging them, so computing the next value from a stale render
     *  closure can silently clobber another toggle's just-written change. */
    patchAppSettings: (patch: SettingsPatch | ((latest: AppSettings | null) => SettingsPatch)) => Promise<void>;
    /** Re-read settings.json from Rust. Lightweight (~1ms). Use after bulk
     *  backend operations (e.g. Fix Everything) so the radar auto-updates. */
    refreshSettings: () => Promise<void>;

    // Dependency status — from centralized dependency module
    dependencyStatus: DependencyInfo[] | null;
    /** Seconds since the last full engine probe. 0 = just probed; null = not yet loaded. */
    depCacheAge: number | null;
    /** Force a fresh engine probe, bypassing the 12-hour file cache. */
    forceRefreshDeps: () => Promise<void>;

    // Status flags
    loading: {
        system: boolean;
        hardening: boolean;
        privacy: boolean;
        network: boolean;
        apps: boolean;
        mesh: boolean;
        dashboard: boolean;
        vault: boolean;
    };

    startupComplete: boolean;
    /** Whether startup surfaces cached settings, a fresh probe, or stale data. */
    startupDataState: 'loading' | 'cached' | 'refreshing' | 'ready' | 'stale';

    // Actions
    refreshAll: () => Promise<void>;
    refreshSystem: (silent?: boolean) => Promise<void>;
    refreshHardening: () => Promise<void>;
    refreshPrivacy: (silent?: boolean) => Promise<void>;
    refreshDashboard: (silent?: boolean) => Promise<void>;
    refreshNetwork: (silent?: boolean, force?: boolean) => Promise<void>;
    refreshMesh: (silent?: boolean) => Promise<void>;
    /** Optimistically flip the cached "is mesh installed?" flag without
     *  waiting for a fresh probe round-trip. Used right after the mesh
     *  installer reports success — the subsequent refreshMesh probe can
     *  race with the OS still registering tailscale.exe and report
     *  "not installed", which makes the UI flash "NOT DETECTED" at the
     *  exact moment the task popup says "INSTALLED". */
    markMeshInstalled: (installed: boolean) => void;
    refreshBranding: () => Promise<void>;
    refreshVault: (silent?: boolean) => Promise<EncryptionStatus | null>;
    refreshProductivity: (silent?: boolean) => Promise<void>;
    /** Run the unified app inventory scan (Get-AppInventory).
     * Auto-persists to settings.json → current.apps.inventory.
     * Called on startup, every 60 min, and after install/upgrade/uninstall. */
    runAppInventoryScan: (silent?: boolean) => Promise<void>;
    /** Refresh dependency status (Get-DependencyStatus). Called on startup. */
    refreshDependencies: (silent?: boolean) => Promise<void>;
    /** Refresh SMART health for visible logical drives using smartctl CLI. */
    refreshDriveHealth: () => Promise<void>;
    runStartupJob: <T>(job: StartupJob<T>) => Promise<StartupJobResult<T>>;
}

const AppContext = createContext<AppState | null>(null);

// Module-level cache for the once-per-lifetime SMART probe — survives any
// setSystemInfo call that replaces `disks` (notably initializeApp's
// Get-StartupStatus result, which doesn't include SMART data). Lookup keys
// are normalised drive letters ("C:", "D:"). Updated by refreshDriveHealth
// and read by every setSystemInfo merge below.
const smartHealthCache: Record<string, number | null> = {};

// True once seedFromCachedSettings has seeded systemInfo from a populated
// current.device.* cache. Drives the lazy Get-SystemInfo refresh delay: when
// the cache is warm (normal launch) we defer 15s; on a cold first run (no
// cache) we fetch quickly so CPU-model/GPU/hostname labels don't sit blank.
let deviceCacheWarm = false;

// Module-level timestamp of the last successful network (DNS + blocklist) probe.
// Used by refreshNetwork to skip redundant DNS + blocklist probes while the
// Network panel is active. A `force` argument (manual refresh / post-mutation
// callers) bypasses this guard.
const NETWORK_TTL_MS = 300_000; // 5 min
let lastNetworkFetch = 0;
const APP_INVENTORY_SOFT_TIMEOUT_MS = 45_000;

// Recursively strip null/undefined leaves from a probe patch object.
// When a probe script doesn't detect a setting it returns null/undefined.
// Writing null via patch_settings_cmd would OVERWRITE a valid `true` set by
// another probe (merge_json replaces any value with null). Stripping null
// leaves from the patch instead causes merge_json to SKIP the field —
// preserving whatever the authoritative probe (settings-bridge.ps1) wrote.
function stripNullLeaves(obj: unknown): Record<string, unknown> | undefined {
    if (obj === null || obj === undefined || typeof obj !== 'object' || Array.isArray(obj)) return undefined;
    const result: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(obj as Record<string, unknown>)) {
        if (val === null || val === undefined) continue;
        if (typeof val === 'object' && !Array.isArray(val)) {
            const nested = stripNullLeaves(val);
            if (nested !== undefined) result[key] = nested;
        } else {
            result[key] = val;
        }
    }
    return Object.keys(result).length > 0 ? result : undefined;
}

export const AppProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const { mode: authMode } = useAuthMode();
    const { data: startupLicense } = useLicenseQuery();
    const startupCoordinatorRef = useRef<StartupCoordinator | null>(null);
    const systemProbeStoreRef = useRef(createStartupProbeStore<unknown>());
    const startupStatusStoreRef = useRef(createStartupProbeStore<unknown>());
    const {
        getSystemInfo,
        getDriveSmartHealth,
        getStartupStatus,
        getMeshVPNStatus,
        getDefenderStatus,
        getUpdateStatus,
        getBlocklistStatus,
        getDNSStatus,
        getAppBranding,
        getEncryptedVolumeStatus,
        getProductivityStatus,
        getAppInventory,
    } = useBackend();
    if (!startupCoordinatorRef.current) {
        startupCoordinatorRef.current = createStartupCoordinator({
            reportToNative: createTauriStartupReporter((command, args) => invoke(command, args)),
        });
    }
    const runStartupJob = useCallback(<T,>(job: StartupJob<T>) => startupCoordinatorRef.current!.run(job), []);

    // Data State
    const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null);
    const [meshInstalled, setMeshInstalled] = useState<boolean | null>(null);
    const [meshStatus, setMeshStatus] = useState<MeshVPNStatus | null>(null);
    const [defenderStatus, setDefenderStatus] = useState<DefenderStatus | null>(null);
    const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
    const [networkBlocklistStatus, setNetworkBlocklistStatus] = useState<BlocklistStatus | null>(null);
    const [networkDnsStatus, setNetworkDnsStatus] = useState<DNSStatus | null>(null);
    const [encryptionStatus, setEncryptionStatus] = useState<EncryptionStatus | null>(null);
    const [productivityStatus, setProductivityStatus] = useState<{ installed?: boolean; running: boolean; details: { server: boolean; input: boolean; active: boolean } } | null>(null);

    // Unified Settings State
    const [appSettings, setAppSettings] = useState<AppSettings | null>(null);
    const appSettingsRef = useRef<AppSettings | null>(null);
    useEffect(() => { appSettingsRef.current = appSettings; }, [appSettings]);
    const patchChainRef = useRef<Promise<unknown>>(Promise.resolve());
    const appInventory: AppInventorySnapshot | null = appSettings?.current?.apps?.inventory ?? null;

    // React Query client — kept in sync with AppContext so panels that read
    // via useSettingsQuery (e.g. ToggleSection) see patches immediately
    // instead of waiting for the 30s staleTime to expire.
    const queryClient = useQueryClient();

    // Dependency status — from centralized dependency module
    const [dependencyStatus, setDependencyStatus] = useState<DependencyInfo[] | null>(null);
    const [depCacheAge, setDepCacheAge] = useState<number | null>(null);
    const dependencyStatusRef = useRef<DependencyInfo[] | null>(null);
    // KT: This tracks the real backend Get-AppInventory completion, not just
    // the caller's UI wait. A soft timeout can return while winget is still
    // running; clearing the guard there lets duplicate scans stack up.
    const appInventoryBackendInFlightRef = useRef<Promise<void> | null>(null);

    // Loading State
    const [loading, setLoading] = useState({
        system: true,
        hardening: true,
        privacy: true,
        network: true,
        apps: false,
        mesh: false,
        dashboard: true,
        vault: false,
    });

    const [startupComplete, setStartupComplete] = useState(false);
    const [startupDataState, setStartupDataState] = useState<'loading' | 'cached' | 'refreshing' | 'ready' | 'stale'>('loading');

    const normalizeModulesConfig = useCallback((
        modules: ModuleConfig | undefined,
        level: 'simple' | 'standard' | 'advanced' | undefined
    ): ModuleConfig => {
        const base = getDefaultModules(level ?? 'standard');
        return { ...base, ...(modules ?? {}) };
    }, []);

    const normalizeDriveLetter = useCallback((value: string | null | undefined): string | null => {
        if (!value) return null;
        const match = value.toUpperCase().match(/[A-Z]:/);
        return match ? match[0] : null;
    }, []);

    const mergeDiskHealth = useCallback((
        disks: any,
        healthByDrive: Record<string, number | null>
    ) => {
        const disksArray = Array.isArray(disks) ? disks : (disks ? [disks] : []);
        return disksArray.map((disk: any) => {
            const letter = normalizeDriveLetter(disk.label) ?? normalizeDriveLetter(disk.id);
            const healthPercent = letter ? (healthByDrive[letter] ?? null) : null;
            return {
                ...disk,
                healthPercent,
            };
        });
    }, [normalizeDriveLetter]);

    const refreshSystem = useCallback(async (silent: boolean = false) => {
        if (!silent) setLoading(prev => ({ ...prev, system: true }));
        try {
            const res = await getSystemInfo();
            if (res.success && res.data) {
                const data = res.data;
                setSystemInfo(prev => {
                    const healthByDrive: Record<string, number | null> = {};
                    const disksArray = Array.isArray(prev?.disks) ? prev.disks : (prev?.disks ? [prev.disks] : []);
                    disksArray.forEach((disk: any) => {
                        const letter = normalizeDriveLetter(disk.label) ?? normalizeDriveLetter(disk.id);
                        if (!letter) return;
                        healthByDrive[letter] = disk.healthPercent ?? null;
                    });
                    return {
                        ...data,
                        disks: mergeDiskHealth(data.disks ?? [], healthByDrive),
                    };
                });
            }
        } finally {
            if (!silent) setLoading(prev => ({ ...prev, system: false }));
        }
    }, [getSystemInfo, mergeDiskHealth, normalizeDriveLetter]);

    const refreshDriveHealth = useCallback(async () => {
        try {
            const result = await getDriveSmartHealth();
            const data = result as DriveSmartHealthResult;
            if (!data || !data.drives) return;

            const healthByDrive: Record<string, number | null> = {};
            const drivesArray = Array.isArray(data.drives) ? data.drives : [data.drives];
            drivesArray.forEach((drive: any) => {
                const letter = normalizeDriveLetter(drive.driveLetter);
                if (!letter) return;
                healthByDrive[letter] = drive.healthPercent ?? null;
                // Persist to the module cache so any later setSystemInfo
                // replace can re-merge the SMART data — needed because
                // initializeApp's Get-StartupStatus result erases disks
                // (no SMART) and otherwise we'd lose the health %.
                smartHealthCache[letter] = drive.healthPercent ?? null;
            });

            setSystemInfo(prev => {
                if (!prev) return prev;
                return {
                    ...prev,
                    disks: mergeDiskHealth(prev.disks ?? [], healthByDrive),
                };
            });
        } catch {
            // smartctl unavailable or probe failed — leave current values unchanged
        }
    }, [getDriveSmartHealth, mergeDiskHealth, normalizeDriveLetter]);

    // ────────────────────────────────────────────────────────────────────
    // persistProbeToSettings — Write fresh probe data back to settings.json
    // ────────────────────────────────────────────────────────────────────
    // KT: The consolidated startup probe (Get-StartupStatus) returns rich
    // data including app capabilities, but Get-WCSystemProbe doesn't probe
    // those. This means capabilities stay null in settings.json → current.
    // We fix this by persisting ALL probe results back to current after each
    // live fetch. Next startup seedFromCachedSettings will have real data.
    // ────────────────────────────────────────────────────────────────────
    const persistProbeToSettings = useCallback(async (data: any) => {
        try {
            const h = data.hardeningStatus as HardeningStatus | undefined;
            const caps = data.appPrivacyCapabilities as AppPrivacyCapabilitiesStatus | undefined;
            const capMap = (v: boolean | undefined | null): string | null =>
                v === true ? 'Deny' : v === false ? 'Allow' : null;

            const patch: any = {
                current: {
                    // Only write device fields when systemInfo is present in this probe.
                    // Omitting it (lazy refresh path) must not overwrite the cached values.
                    ...(data.systemInfo ? {
                        device: {
                            hostname: data.systemInfo.hostname ?? null,
                            osName: data.systemInfo.osName ?? null,
                            osVersion: data.systemInfo.osVersion ?? null,
                            buildNumber: data.systemInfo.buildNumber ?? null,
                            deviceType: data.systemInfo.deviceType ?? null,
                            cpu: data.systemInfo.cpu ?? null,
                            gpu: data.systemInfo.gpu ?? null,
                            ram: data.systemInfo.ram ?? null,
                            isAdmin: data.systemInfo.isAdmin ?? null,
                        },
                    } : {}),
                    privacy: {
                        telemetry: {
                            windowsDisabled: data.telemetryStatus?.blocked ?? null,
                            officeDisabled: data.officePrivacy?.disabled ?? null,
                            activityHistoryDisabled: h?.activityHistoryDisabled ?? null,
                            locationTrackingDisabled: h?.locationTrackingDisabled ?? null,
                            windowsSuggestionsDisabled: data.windowsSuggestions?.disabled ?? null,
                            powershell7Disabled: h?.poshTelemetryDisabled ?? null,
                        },
                        clipboard: {
                            historyDisabled: data.clipboardHistory?.clipboardHistoryDisabled ?? null,
                            cloudSyncDisabled: data.clipboardHistory?.cloudClipboardDisabled ?? null,
                            // autoEraseSchedule moved to the Privacy Clean panel's
                            // per-card scheduler (Set-AutoEraseSchedule). No longer
                            // a single-toggle state in app settings.
                        },
                        tracking: {
                            // ?? undefined (not null): an UNPROBED radar toggle must
                            // surface in "Needs Attention" ("not configured"), not be
                            // skipped as "not supported on this system" (null → skip in
                            // radarScan). stripNullLeaves treats both as "don't persist".
                            recentFilesDisabled: data.privacyProtection?.recentFiles ?? undefined,
                            jumpListsDisabled: data.privacyProtection?.jumpLists ?? undefined,
                            thumbnailCacheDisabled: data.privacyProtection?.thumbnailCache ?? undefined,
                            pagefileDisabled: data.privacyProtection?.pagefile ?? null,
                            // rdpAutoEraseSchedule + eventLogAutoEraseSchedule moved
                            // to the Privacy Clean panel's per-card scheduler.
                            recallSnapshotsDisabled: h?.recallSnapshotsDisabled ?? undefined,
                            typingInsightsDisabled: h?.typingInsightsDisabled ?? undefined,
                            advertisingIdDisabled: h?.advertisingIdDisabled ?? undefined,
                            tailoredExperiencesDisabled: h?.tailoredExperiencesDisabled ?? undefined,
                            officeLoggingDisabled: h?.officeLoggingDisabled ?? undefined,
                            diagnosticEventTracingDisabled: h?.diagnosticEventTracingDisabled ?? undefined,
                            // Phase E — hide-recent MRU surfaces (HKCU reads from Get-HardeningStatus).
                            quickAccessRecentDisabled: h?.hideQuickAccessRecent ?? undefined,
                            quickAccessFrequentDisabled: h?.hideQuickAccessFrequent ?? undefined,
                            runMruDisabled: h?.hideRunMRU ?? undefined,
                            searchHistoryDisabled: h?.disableSearchHistory ?? undefined,
                            terminalHistoryDisabled: h?.terminalHistoryDisabled ?? undefined,
                        },
                        internetCommunication: {
                            restrictedEnabled: h?.internetCommRestricted ?? null,
                        },
                        lockscreen: {
                            privacyDisabled: data.lockScreenPrivacy?.disabled ?? null,
                        },
                        setupCompletionNagsDisabled: data.setupNags?.disabled ?? null,
                        ...(caps ? {
                            appCapabilities: {
                                webcam: capMap(caps.webcam),
                                microphone: capMap(caps.microphone),
                                contacts: capMap(caps.contacts),
                                calendar: capMap(caps.appointments),
                                callHistory: capMap(caps.phoneCallHistory),
                                phoneCall: capMap(caps.phoneCall),
                                // KT: location, email, radios aren't in the PS probe
                                // (Get-AppPrivacyCapabilitiesStatus returns 16 caps, not these 3).
                                // Default to "Allow" so they're never stuck as null.
                                location: 'Allow',
                                email: 'Allow',
                                radios: 'Allow',
                                messaging: capMap(caps.chat),
                                notifications: capMap(caps.userNotificationListener),
                                documents: capMap(caps.documentsLibrary),
                                pictures: capMap(caps.picturesLibrary),
                                videos: capMap(caps.videosLibrary),
                                fileSystem: capMap(caps.broadFileSystemAccess),
                                gazeInput: capMap(caps.gazeInput),
                                appDiagnostics: capMap(caps.appDiagnostics),
                                userAccountInformation: capMap(caps.userAccountInformation),
                                bluetoothSync: capMap(caps.bluetoothSync),
                            },
                        } : {}),
                    },
                    tweaks: {
                        security: {
                            defenderDisabled: h?.defenderDisabled ?? null,
                            windowsUpdateDisabled: h?.updatesPaused ?? null,
                            uacDisabled: h?.uacDisabled ?? null,
                            usbWriteProtect: h?.usbWriteProtect ?? null,
                            usbStorageLockdown: h?.usbStorageLockdown ?? null,
                            consumerFeaturesDisabled: h?.consumerFeaturesDisabled ?? null,
                            // Host hardening (Feature 4)
                            systemRestoreOff: h?.systemRestoreOff ?? null,
                            recallOff: h?.recallSnapshotsDisabled ?? null,
                            crashDumpsOff: h?.crashDumpsOff ?? null,
                            clipboardHistoryOff: h?.clipboardHistoryOff ?? null,
                            requirePwOnResume: h?.requirePwOnResume ?? null,
                            kernelDmaProtect: h?.kernelDmaProtect ?? null,
                            // RAM-spill control (Feature 3)
                            ramSpillControlEnabled: h?.ramSpillControl ?? null,
                            // Anti-Acquisition Defenses
                            acquisitionDriverBlocklist: h?.acquisitionDriverBlocklist ?? null,
                            forensicToolBlock: h?.forensicToolBlock ?? null,
                            lidClosePowerOff: h?.lidClosePowerOff ?? null,
                            // Exploit Protection (Set-ProcessMitigation)
                            depEnabled: h?.depEnabled ?? null,
                            aslrMandatory: h?.aslrMandatory ?? null,
                            aslrBottomUp: h?.aslrBottomUp ?? null,
                            cfgEnabled: h?.cfgEnabled ?? null,
                            heapIntegrity: h?.heapIntegrity ?? null,
                            sehopEnabled: h?.sehopEnabled ?? null,
                            asrRulesEnabled: h?.asrRulesEnabled ?? null,
                            controlledFolderAccessEnabled: h?.controlledFolderAccessEnabled ?? null,
                            networkProtectionEnabled: h?.networkProtectionEnabled ?? null,
                        },
                        os: {
                            superfetchDisabled: h?.superfetchDisabled ?? null,
                            hibernationDisabled: h?.hibernationDisabled ?? null,
                            fastStartupDisabled: h?.fastStartupDisabled ?? null,
                            ntfsOptimizationsEnabled: h?.ntfsOptimizations ?? null,
                            detailedBsodEnabled: h?.detailedBSOD ?? null,
                            prefetchDisabled: data.privacyProtection?.prefetch ?? null,
                        },
                        ui: {
                            classicContextMenu: h?.classicContextMenu ?? null,
                            fileExtensionsVisible: h?.fileExtensionsShown ?? null,
                            hiddenFilesVisible: h?.hiddenFilesShown ?? null,
                            galleryHomeRemoved: h?.galleryHomeRemoved ?? null,
                            bingSearchDisabled: h?.bingSearchDisabled ?? null,
                            backgroundAppsDisabled: h?.backgroundAppsDisabled ?? null,
                            notificationsDisabled: h?.notificationsDisabled ?? null,
                            endTaskOnTaskbar: h?.endTaskOnTaskbar ?? null,
                            explorerOpensThisPc: h?.explorerOpensThisPC ?? null,
                            // ── Granular UI controls ─────────────────────
                            // These come straight from $state.tweaks.ui.* in
                            // Get-WCSystemProbe — there's no h.* equivalent
                            // because Get-HardeningStatus doesn't probe them.
                            desktopIconThisPc: data.tweaks?.ui?.desktopIconThisPc ?? null,
                            desktopIconRecycleBin: data.tweaks?.ui?.desktopIconRecycleBin ?? null,
                            desktopIconUserFiles: data.tweaks?.ui?.desktopIconUserFiles ?? null,
                            desktopIconNetwork: data.tweaks?.ui?.desktopIconNetwork ?? null,
                            desktopIconControlPanel: data.tweaks?.ui?.desktopIconControlPanel ?? null,
                            shortcutArrowRemoved: data.tweaks?.ui?.shortcutArrowRemoved ?? null,
                            snapAssistFlyoutDisabled: data.tweaks?.ui?.snapAssistFlyoutDisabled ?? null,
                            explorerCompactMode: data.tweaks?.ui?.explorerCompactMode ?? null,
                            explorerCheckboxesEnabled: data.tweaks?.ui?.explorerCheckboxesEnabled ?? null,
                            windowShakeDisabled: data.tweaks?.ui?.windowShakeDisabled ?? null,
                            clockSecondsVisible: data.tweaks?.ui?.clockSecondsVisible ?? null,
                        },
                        rdp: {
                            keepAlive: h?.rdpKeepAlive ?? null,
                            noTimeouts: h?.rdpNoTimeouts ?? null,
                            qosPriority: h?.rdpQosPriority ?? null,
                        },
                        // ── Performance / GPU / Power ────────────────────
                        // Pass through wholesale from $state.tweaks.* — the
                        // probe in settings-bridge.ps1 writes the live state,
                        // and there are no equivalent fields on h to fall back
                        // to. ?? null preserves "unprobed" vs "false" so
                        // toggles don't lie when probe data is missing.
                        performance: {
                            mmcssGamingProfile: data.tweaks?.performance?.mmcssGamingProfile ?? null,
                            keyboardLatencyOptimised: data.tweaks?.performance?.keyboardLatencyOptimised ?? null,
                            numLockOnBoot: data.tweaks?.performance?.numLockOnBoot ?? null,
                            gpuSchedulingEnabled: data.tweaks?.performance?.gpuSchedulingEnabled ?? null,
                            svcHostSplitOptimised: data.tweaks?.performance?.svcHostSplitOptimised ?? null,
                            accessibilityShortcutsDisabled: data.tweaks?.performance?.accessibilityShortcutsDisabled ?? null,
                            instantMenuDelay: data.tweaks?.performance?.instantMenuDelay ?? null,
                            mouseAccelerationDisabled: data.tweaks?.performance?.mouseAccelerationDisabled ?? null,
                            autocorrectDisabled: data.tweaks?.performance?.autocorrectDisabled ?? null,
                            enthusiastModeEnabled: data.tweaks?.performance?.enthusiastModeEnabled ?? null,
                            wallpaperFullQuality: data.tweaks?.performance?.wallpaperFullQuality ?? null,
                        },
                        gpu: {
                            amdUlpsDisabled: data.tweaks?.gpu?.amdUlpsDisabled ?? null,
                            amdPowerGatingDisabled: data.tweaks?.gpu?.amdPowerGatingDisabled ?? null,
                            amdVideoClockGatingDisabled: data.tweaks?.gpu?.amdVideoClockGatingDisabled ?? null,
                            amdAspmDisabled: data.tweaks?.gpu?.amdAspmDisabled ?? null,
                            nvidiaDynamicPstateDisabled: data.tweaks?.gpu?.nvidiaDynamicPstateDisabled ?? null,
                            nvidiaAsyncPstatesDisabled: data.tweaks?.gpu?.nvidiaAsyncPstatesDisabled ?? null,
                            intelAsyncFlipsDisabled: data.tweaks?.gpu?.intelAsyncFlipsDisabled ?? null,
                            intelAdaptiveVsyncDisabled: data.tweaks?.gpu?.intelAdaptiveVsyncDisabled ?? null,
                        },
                        power: {
                            usbSelectiveSuspendDisabled: data.tweaks?.power?.usbSelectiveSuspendDisabled ?? null,
                            cpuThrottlingDisabled: data.tweaks?.power?.cpuThrottlingDisabled ?? null,
                        },
                        // Unified power-plan picker (Ultimate is the 4th option).
                        powerPlan: data.tweaks?.powerPlan ?? null,
                    },
                },
            };

            const stripped = stripNullLeaves(patch);
            if (!stripped) return;
            const updated = await invoke<AppSettings>('patch_settings_cmd', { patch: stripped });
            setAppSettings(updated);
            appSettingsRef.current = updated;
            // Mirror into React Query so useAdoptCurrentState fires immediately
            // instead of waiting for the 30s staleTime window.
            queryClient.setQueryData(settingsKeys.detail(), updated);
        } catch {
            // Best-effort — don't break startup if settings write fails
        }
    }, [queryClient]);

    // ────────────────────────────────────────────────────────────────────
    // seedFromCachedSettings — Instant state hydration from settings.json cache
    // ────────────────────────────────────────────────────────────────────
    // KT: settings.json → current already has data from the last probe run.
    // By seeding React state from it (via fast Rust IPC, <5ms), the sovereignty
    // score, dashboard, sidebar, and title bar all render with real values
    // immediately — no waiting for slow PowerShell probes.
    // On first run (no cache), current is all nulls → score naturally = 0.
    // After first probe run, persistProbeToSettings writes real values back,
    // so subsequent launches have full data cached.
    // ────────────────────────────────────────────────────────────────────
    const seedFromCachedSettings = useCallback((settings: AppSettings) => {
        const c = settings.current;
        if (!c) return;

        // ── Defender Status (score uses defenderStatus.realtimeEnabled) ──
        setDefenderStatus({
            serviceRunning: !(c.tweaks?.security?.defenderDisabled ?? false),
            realtimeEnabled: !(c.tweaks?.security?.defenderDisabled ?? false),
        });

        // ── Update Status (score uses updateStatus.paused) ──
        setUpdateStatus({
            serviceRunning: !(c.tweaks?.security?.windowsUpdateDisabled ?? false),
            paused: c.tweaks?.security?.windowsUpdateDisabled ?? false,
            pausedUntil: null,
        });

        // ── Device / System Info (static hardware fields from last probe) ──
        // KT: These are written by persistProbeToSettings after each startup probe.
        // Only seed if hostname is non-null — an empty {}/all-null device block
        // (e.g. first run) must NOT trigger a seed, or the Dashboard shows
        // "Unknown" text instead of the loading skeleton.
        const d = c.device as any;
        const hasRealDeviceCache = !!d?.hostname;
        if (hasRealDeviceCache) deviceCacheWarm = true;
        if (hasRealDeviceCache) {
            setSystemInfo(prev => ({
                // Preserve live volatile fields already populated by refreshSystem / refreshLiveMetrics
                // so re-seeding from cache (e.g. after background probe completes) doesn't zero them out.
                cpuUsage: prev?.cpuUsage ?? 0,
                cpuTemp: prev?.cpuTemp ?? 0,
                ramUsage: prev?.ramUsage ?? 0,
                disks: prev?.disks ?? [],
                uptime: prev?.uptime ?? { days: 0, hours: 0, minutes: 0 },
                // Static device fields from cache:
                hostname: d.hostname ?? "Unknown",
                osName: d.osName ?? "Windows",
                osVersion: d.osVersion ?? "",
                buildNumber: d.buildNumber ?? "",
                deviceType: d.deviceType ?? "Unknown",
                isAdmin: d.isAdmin ?? false,
                cpu: d.cpu ?? "Unknown",
                gpu: d.gpu ?? "Unknown",
                ram: d.ram ?? "Unknown",
            }));
        }

        // KT: Clear sovereignty-critical loading flags so score renders instantly.
        // loading.system cleared only when real device cache exists.
        setLoading(prev => ({
            ...prev,
            system: hasRealDeviceCache ? false : prev.system,
            hardening: false,
            privacy: false,
            network: false,
            dashboard: false,
        }));
    }, []);

    const initializeApp = useCallback(async (signal?: AbortSignal): Promise<boolean> => {
        // Consolidated startup call - fetches System, Hardening, Privacy, and Essential Apps in one go
        // A warm settings cache is usable. Keep it on screen while probes refresh
        // rather than replacing it with skeletons a second time.
        if (!appSettingsRef.current) {
            setLoading(prev => ({ ...prev, system: true, hardening: true, privacy: true, apps: true }));
        } else {
            setLoading(prev => ({ ...prev, apps: true }));
        }
        try {
            const activeSignal = signal ?? new AbortController().signal;
            const res = await startupStatusStoreRef.current.refresh(
                () => getStartupStatus(),
                activeSignal,
            ) as Awaited<ReturnType<typeof getStartupStatus>>;
            if (activeSignal.aborted) return false;
            if (res.success && res.data) {
                // systemInfo is omitted from Get-StartupStatus — static hardware fields
                // (cpu, gpu, ram, hostname) are already in the UI via seedFromCachedSettings.
                // A lazy 15s background refresh updates them after startup.
                const incoming = res.data.systemInfo;
                if (incoming) {
                    setSystemInfo({
                        ...incoming,
                        disks: mergeDiskHealth(incoming.disks ?? [], smartHealthCache),
                    });
                }

                if (res.data.productivity) {
                    setProductivityStatus(res.data.productivity as any);
                }

                // KT: Persist all fresh probe data back to settings.json → current.
                // persistProbeToSettings writes everything (hardening, privacy,
                // telemetry, caps, etc.) so the SSOT is settings.json.
                // Individual React state vars are gone — UI reads appSettings directly.
                await persistProbeToSettings(res.data);
            }
            return res.success && Boolean(res.data);
        } catch {
            return false;
        } finally {
            setLoading(prev => ({ ...prev, system: false, hardening: false, privacy: false, apps: false }));
        }
    }, [getStartupStatus, mergeDiskHealth, persistProbeToSettings]);

    /** Lightweight re-read of settings.json from Rust (~1ms). Does NOT
     *  trigger any PowerShell probe — just reads what Rust already has. */
    const refreshSettings = useCallback(async () => {
        // KT: decoy guard — reading real settings here would reseed appSettings
        // with the real config, blowing the fake-data substitution in the ctx memo.
        if (authMode === 'decoy') return;
        try {
            const updated = await invoke<AppSettings>('get_settings');
            // PERF: the active-panel poller calls this every 10s just to READ
            // settings — it mutates nothing. Re-seeding React state with a fresh
            // object identity re-renders the whole app (every useAppState
            // consumer), which can hitch an in-progress scroll. Skip the update
            // entirely when the settings are unchanged (the common case): only
            // re-render when a background probe actually wrote something new.
            const prev = appSettingsRef.current;
            if (prev && JSON.stringify(prev) === JSON.stringify(updated)) return;
            appSettingsRef.current = updated;
            setAppSettings(updated);
            queryClient.setQueryData(settingsKeys.detail(), updated);
            seedFromCachedSettings(updated);
        } catch (err) {
            console.error('Failed to refresh settings:', err);
        }
    }, [authMode, queryClient, seedFromCachedSettings]);

    const refreshHardening = useCallback(async () => {
        // Thin wrapper — all hardening data is in settings.json, written by sync patches.
        // refreshSettings() reads it from Rust in ~1ms (zero PS processes).
        await refreshSettings();
    }, [refreshSettings]);

    const refreshPrivacy = useCallback(async (_silent: boolean = false) => {
        // Thin wrapper — all privacy data is in settings.json, written by sync patches.
        // refreshSettings() reads it from Rust in ~1ms (zero PS processes).
        await refreshSettings();
    }, [refreshSettings]);

    const refreshAll = useCallback(async () => {
        // Legacy individual refresh if needed, but initializeApp is preferred for startup
        await Promise.all([
            refreshSystem(),
            refreshHardening(),
            refreshPrivacy()
        ]);
    }, [refreshSystem, refreshHardening, refreshPrivacy]);

    const refreshDashboard = useCallback(async (silent: boolean = false) => {
        if (!silent) setLoading(prev => ({ ...prev, dashboard: true }));
        try {
            const [defender, updates] = await Promise.all([
                getDefenderStatus(),
                getUpdateStatus()
            ]);
            if (defender.success && defender.data) setDefenderStatus(defender.data);
            if (updates.success && updates.data) setUpdateStatus(updates.data);

            // KT: Sync fresh defender/update data back to settings.json → current
            // so next startup seed has real values for score calculation.
            try {
                const dashPatch: any = { current: { tweaks: { security: {} } } };
                if (defender.success && defender.data) {
                    dashPatch.current.tweaks.security.defenderDisabled = !defender.data.realtimeEnabled;
                }
                if (updates.success && updates.data) {
                    dashPatch.current.tweaks.security.windowsUpdateDisabled = updates.data.paused;
                }
                await invoke('patch_settings_cmd', { patch: dashPatch });
            } catch { /* best-effort */ }
        } finally {
            if (!silent) setLoading(prev => ({ ...prev, dashboard: false }));
        }
    }, [getDefenderStatus, getUpdateStatus]);

    const refreshNetwork = useCallback(async (silent: boolean = false, force: boolean = false) => {
        // Staleness guard: active Network-panel polling should not re-probe DNS +
        // blocklists on every tick. Manual refresh and post-mutation callers force
        // a fresh read.
        if (!force && lastNetworkFetch !== 0 && Date.now() - lastNetworkFetch < NETWORK_TTL_MS) {
            return;
        }
        if (!silent) setLoading(prev => ({ ...prev, network: true }));
        try {
            const [blocklists, dns] = await Promise.all([
                getBlocklistStatus(),
                getDNSStatus()
            ]);
            if (blocklists.success && blocklists.data) setNetworkBlocklistStatus(blocklists.data);
            if (dns.success && dns.data) setNetworkDnsStatus(dns.data);
            lastNetworkFetch = Date.now();

            // KT: Sync fresh network data back to settings.json → current
            try {
                const netPatch: any = { current: { network: {} } };
                if (dns.success && dns.data) {
                    netPatch.current.network.dns = {
                        provider: dns.data.provider ?? null,
                        ipv4Preference: dns.data.dohId ? true : false,
                    };
                }
                if (blocklists.success && blocklists.data) {
                    netPatch.current.network.hosts = {
                        enabledBlocklists: blocklists.data.applied ?? [],
                    };
                }
                await invoke('patch_settings_cmd', { patch: netPatch });
            } catch { /* best-effort */ }
        } finally {
            if (!silent) setLoading(prev => ({ ...prev, network: false }));
        }
    }, [getBlocklistStatus, getDNSStatus]);

    // ────────────────────────────────────────────────────────────────────
    // runAppInventoryScan — Unified app inventory scan
    // ────────────────────────────────────────────────────────────────────
    // LEARNING: This replaces the need to parallelise Get-AppStatus + Get-UpgradeList +
    // Get-EssentialAppsStatus. One call does everything and the Rust backend auto-persists
    // the result to settings.json → current.apps.inventory.
    //
    // TRIGGERS:
    //   1. App startup (after settings load, delayed so it doesn't block UI)
    //   2. Every 60 minutes (configurable via ideal.apps.scanIntervalMinutes)
    //   3. After install/upgrade/uninstall actions (caller invokes this)
    //   4. Manual refresh button in Apps panel
    // ────────────────────────────────────────────────────────────────────
    const runAppInventoryScan = useCallback(async (silent: boolean = false) => {
        const startBackendScan = () => {
            const task = Promise.resolve()
                .then(() => getAppInventory())
                .then(async (res) => {
                    if (res.success && res.data) {
                        // Re-read settings to get the updated inventory that backend auto-persisted
                        try {
                            const updatedSettings = await invoke<AppSettings>('get_settings');
                            setAppSettings(updatedSettings);
                            appSettingsRef.current = updatedSettings;
                            seedFromCachedSettings(updatedSettings);
                        } catch { /* best-effort */ }
                    } else if (!res.success) {
                        console.warn('App inventory scan did not return data:', res.error ?? res);
                    }
                })
                .catch((err) => {
                    console.error('App inventory scan failed:', err);
                })
                .finally(() => {
                    if (appInventoryBackendInFlightRef.current === task) {
                        appInventoryBackendInFlightRef.current = null;
                    }
                });

            appInventoryBackendInFlightRef.current = task;
            return task;
        };

        const scanTask = appInventoryBackendInFlightRef.current ?? startBackendScan();
        if (!silent) setLoading(prev => ({ ...prev, apps: true }));
        try {
            const result = await waitForSoftTimeout(scanTask, APP_INVENTORY_SOFT_TIMEOUT_MS);
            if (result.status === "timed-out") {
                console.warn('App inventory scan is still running after 45 seconds; keeping cached inventory and applying results when it finishes.');
            }
        } finally {
            if (!silent) setLoading(prev => ({ ...prev, apps: false }));
        }
    }, [getAppInventory, seedFromCachedSettings]);

    const refreshMesh = useCallback(async (silent: boolean = false) => {
        if (!silent) setLoading(prev => ({ ...prev, mesh: true }));
        try {
            const res = await getMeshVPNStatus();
            if (res.success && res.data) {
                setMeshStatus(res.data);
                setMeshInstalled(!!res.data.installed);
            }
        } finally {
            if (!silent) setLoading(prev => ({ ...prev, mesh: false }));
        }
    }, [getMeshVPNStatus]);

    const markMeshInstalled = useCallback((installed: boolean) => {
        setMeshInstalled(installed);
    }, []);

    const refreshVault = useCallback(async (silent: boolean = false): Promise<EncryptionStatus | null> => {
        if (!silent) setLoading(prev => ({ ...prev, vault: true }));
        try {
            // This Pro-native endpoint returns only drive links in the current
            // logon session. The legacy VeraCrypt/PowerShell probe is machine-
            // scoped and must not be used as a fallback on multi-user hosts.
            const res = await getEncryptedVolumeStatus();
            if (res.success && res.data) {
                setEncryptionStatus(res.data);
                return res.data;
            }
            // Do not leave an old mount list on-screen after a failed probe.
            // A stale badge can otherwise claim a drive exists when Explorer cannot see it.
            setEncryptionStatus(null);
            return null;
        } finally {
            if (!silent) setLoading(prev => ({ ...prev, vault: false }));
        }
    }, [getEncryptedVolumeStatus]);

    const refreshProductivity = useCallback(async (silent: boolean = false) => {
        if (!silent) setLoading(prev => ({ ...prev, dashboard: true }));
        try {
            const res = await getProductivityStatus();
            if (res.success && res.data) setProductivityStatus(res.data as any);
        } finally {
            if (!silent) setLoading(prev => ({ ...prev, dashboard: false }));
        }
    }, [getProductivityStatus]);

    const refreshDependencies = useCallback(async (_silent: boolean = false, force = false) => {
        // Dependency check runs silently — no operation card shown to the user.
        // Status is stored in context and consumed by the panels that need it.
        try {
            const res = await executeBackendCommand<{ dependencies: any; cacheAgeSecs?: number }>(
                'Get-DependencyStatus',
                force ? { Force: true } : {}
            );
            if (res.success && res.data?.dependencies) {
                const depsArray = Array.isArray(res.data.dependencies) ? res.data.dependencies : [res.data.dependencies];
                setDependencyStatus(depsArray);
                setDepCacheAge(res.data.cacheAgeSecs ?? 0);
                dependencyStatusRef.current = depsArray;

                // Auto-start deps that are installed but not running
                for (const dep of depsArray) {
                    if (dep.installed && dep.canStart && dep.running === false) {
                        console.log(`[Dependencies] Auto-starting ${dep.name}…`);
                        executeBackendCommand('Install-Dependency', { Id: dep.id })
                            .catch(err => console.error(`[Dependencies] Auto-start ${dep.id} failed:`, err));
                    }
                }
            }
        } catch (err) {
            console.error('Dependency status check failed:', err);
        }
    }, []);

    const forceRefreshDeps = useCallback(() => refreshDependencies(false, true), [refreshDependencies]);

    const refreshBranding = useCallback(async () => {
        // settings.json is the SOURCE OF TRUTH for branding — set by the
        // Identity panel's Apply button via patchAppSettings. The
        // registry is a side-effect mirror for external tools that
        // might want to read it. Previously this function read registry
        // → patched settings, which OVERWROTE the user's saved branding
        // with defaults whenever the registry path didn't exist or was
        // cleared. Now we only sync settings → registry (one-way), so
        // settings.json wins on every load.
        const current = await invoke<AppSettings>('get_settings').catch(() => null);
        if (!current) return;
        const company = current.ideal?.identity?.branding?.companyName?.trim();
        const product = current.ideal?.identity?.branding?.productName?.trim();
        // Only push to registry if settings actually has values; never
        // overwrite registry with empty/null.
        if (company || product) {
            try {
                await getAppBranding(); // probe (no-op if backend unavailable)
                // Setter via Set-AppBranding so external tools see the
                // same values the user picked.
                await invoke('run_backend_script', {
                    command: 'Set-AppBranding',
                    params: {
                        CompanyName: company || 'ServaLabs',
                        ProductName: product || 'WinCommander',
                    },
                }).catch(() => {});
            } catch { /* best-effort sync; settings.json remains authoritative */ }
        }
    }, [getAppBranding]);

    // Guard: prevent double-execution of the heavy system probe on React re-mount
    const probeRanRef = useRef(false);

    // Unified Settings: Load on init, run system probe, populate ideal+current
    const initSettings = useCallback(async (runProbe = true, cachedSettings?: AppSettings, signal?: AbortSignal): Promise<AppSettings | null> => {
        try {
            // Phase 1: Fast cache read (~1-5ms) — seeds state for instant score
            let settings = cachedSettings ?? await invoke<AppSettings>('get_settings');
            if (signal?.aborted) return null;

            // Heal sparse/legacy module maps so missing keys do NOT default to false on restart.
            const currentLevel = settings.app?.experienceLevel ?? 'standard';
            const normalizedModules = normalizeModulesConfig(settings.app?.modules, currentLevel);
            const hadModuleShapeDrift = Object.keys(normalizedModules).length !== Object.keys(settings.app?.modules ?? {}).length;
            if (hadModuleShapeDrift) {
                settings = await invoke<AppSettings>('patch_settings_cmd', {
                    patch: { app: { modules: normalizedModules } },
                });
            }

            setAppSettings(settings);
            appSettingsRef.current = settings;
            seedFromCachedSettings(settings);
            setStartupDataState('cached');

            // Phase 2: System probe runs after the cache hydration so startup is
            // responsive on both first and subsequent launches.
            // Guard: skip if already running (React strict mode / dep-change re-fire)
            if (runProbe && !probeRanRef.current) {
                probeRanRef.current = true;

                if (!settings.app.firstRunComplete) {
                    // First launch: the Dashboard and the auto-started first-run tour
                    // data asynchronously. The shell is safe with its explicit
                    // loading state while this slow PowerShell work completes.
                    await (async () => {
                        let probeRes: unknown = null;
                        try {
                            probeRes = await invoke<unknown>('run_backend_script', {
                                command: 'Get-WCSystemProbe',
                                params: {},
                            });
                        } catch {
                            return;
                        }
                        if (signal?.aborted) return;

                        if (probeRes && typeof probeRes === 'object') {
                            // First run: probe populates BOTH ideal and current.
                            const updated = await invoke<AppSettings>('patch_settings_cmd', {
                                patch: { ideal: probeRes, current: probeRes },
                            });
                            setAppSettings(updated);
                            appSettingsRef.current = updated;
                            seedFromCachedSettings(updated);

                            // Overlay migration data on ideal (catches branding, etc.)
                            try {
                                const migrationRes = await invoke<unknown>('run_backend_script', {
                                    command: 'Get-WCMigrationData',
                                    params: {},
                                });
                                if (migrationRes && typeof migrationRes === 'object') {
                                    const merged = await invoke<AppSettings>('patch_settings_cmd', {
                                        patch: { ideal: migrationRes },
                                    });
                                    setAppSettings(merged);
                                    appSettingsRef.current = merged;
                                    seedFromCachedSettings(merged);
                                }
                            } catch {
                                // Migration is best-effort
                            }
                        }
                    })();
                } else {
                    // Subsequent runs: fire-and-forget — UI renders from cached current.* immediately
                    await invoke<unknown>('run_backend_script', {
                        command: 'Get-WCSystemProbe',
                        params: {},
                    })
                        .then(async (probeRes) => {
                            if (signal?.aborted) return;
                            if (probeRes && typeof probeRes === 'object') {
                                const updated = await invoke<AppSettings>('update_current_state', {
                                    probe: probeRes,
                                });
                                setAppSettings(updated);
                                appSettingsRef.current = updated;
                                seedFromCachedSettings(updated);
                                // Mirror into React Query so useAdoptCurrentState
                                // fires immediately after the probe completes.
                                queryClient.setQueryData(settingsKeys.detail(), updated);
                            }
                        })
                        .catch(() => {}); // best-effort
                }
            }
            return settings;
        } catch {
            // Settings engine not available
            setStartupDataState('stale');
            return null;
        }
    }, [seedFromCachedSettings, normalizeModulesConfig, queryClient]);

    const patchAppSettings = useCallback(async (patch: SettingsPatch | ((latest: AppSettings | null) => SettingsPatch)) => {
        // KT: decoy guard — writes in decoy mode must never reach the backend;
        // the Rust backstop also blocks them, but preventing the invoke here
        // ensures no observable round-trip that an examiner could log.
        if (authMode === 'decoy') return;
        // Serialize overlapping writes so two rapid toggles can't both read the same
        // stale base and clobber each other; read the latest settings from a ref so
        // module normalization sees the freshest value, not the closure snapshot.
        const run = patchChainRef.current.then(async () => {
          try {
            const latest = appSettingsRef.current;
            // Resolve function-form patches here, at this write's turn in the
            // chain, instead of at call time — so callers deriving a next value
            // from shared collection state (e.g. VisibilityTable's lockedPanelIds/
            // borrowedHidden arrays, which the backend replaces wholesale rather
            // than deep-merging) always compute against the freshest settings
            // rather than the render closure captured when the user clicked.
            let normalizedPatch: SettingsPatch = typeof patch === 'function' ? patch(latest) : patch;

            // Always persist a COMPLETE modules map (defaults + current + patch),
            // so missing keys never behave like implicit OFF on next startup.
            if (normalizedPatch.app?.modules) {
                const level = normalizedPatch.app?.experienceLevel ?? latest?.app?.experienceLevel ?? 'standard';
                const currentModules = normalizeModulesConfig(latest?.app?.modules, level);
                normalizedPatch = {
                    ...normalizedPatch,
                    app: {
                        ...normalizedPatch.app,
                        modules: {
                            ...currentModules,
                            ...normalizedPatch.app.modules,
                        },
                    },
                };
            }

            const updated = await invoke<AppSettings>('patch_settings_cmd', { patch: normalizedPatch });
            appSettingsRef.current = updated;
            setAppSettings(updated);
            // Mirror into React Query cache so consumers using useSettingsQuery
            // (ToggleSection, etc.) re-render with the new state immediately.
            // Without this they keep serving the stale snapshot until staleTime expires.
            queryClient.setQueryData(settingsKeys.detail(), updated);
            // KT: Re-seed all individual state vars so the UI stays in sync with
            // settings.json after every write. Without this, hardeningStatus,
            // telemetryBlocked etc. stay stale until the next background probe.
            seedFromCachedSettings(updated);
          } catch (err) {
            console.error('Failed to patch settings:', err);
            throw err;
          }
        });
        // keep the chain alive even if one write throws
        patchChainRef.current = run.catch(() => {});
        return run;
    }, [authMode, seedFromCachedSettings, normalizeModulesConfig, queryClient]);

    const startupEligibility = useMemo<StartupEligibility>(() => ({
        hasVerifiedPaidEntitlement: (startupLicense?.licensed === true && startupLicense.valid === true)
            || startupLicense?.trial_active === true,
        isProInstalled: false,
        isProConfigured: !import.meta.env.DEV,
        autoUpdateEnabled: appSettings?.app?.autoUpdate ?? true,
        meshEnabled: appSettings?.app?.fleet?.enabled === true,
        dependenciesEnabled: true,
        hasIdleWindow: false,
    }), [appSettings?.app?.autoUpdate, appSettings?.app?.fleet?.enabled, startupLicense]);
    const startupEligibilityRef = useRef(startupEligibility);
    useEffect(() => { startupEligibilityRef.current = startupEligibility; }, [startupEligibility]);

    // Initial Load
    // KT: Run startup as an explicit sequence instead of clock-based phases.
    // That keeps the app from advancing to the next step before the previous
    // step has actually finished on slower machines.
    useEffect(() => {
        let cancelled = false;
        const timers: ReturnType<typeof setTimeout>[] = [];
        const idleCallbacks: number[] = [];

        const scheduleAfter = (delayMs: number, task: () => void) => {
            const timer = setTimeout(() => {
                if (!cancelled) task();
            }, delayMs);
            timers.push(timer);
        };

        const scheduleWhenIdle = (delayMs: number, task: () => void) => {
            scheduleAfter(delayMs, () => {
                if (cancelled) return;
                const win = window as Window & {
                    requestIdleCallback?: (callback: () => void, options?: { timeout: number }) => number;
                    cancelIdleCallback?: (handle: number) => void;
                };
                if (typeof win.requestIdleCallback === 'function') {
                    const handle = win.requestIdleCallback(() => {
                        if (!cancelled) task();
                    }, { timeout: 15_000 });
                    idleCallbacks.push(handle);
                } else {
                    scheduleAfter(2_000, task);
                }
            });
        };

        const runStartupSequence = async () => {
            // Emit a startup phase. Also stash the latest on a window global so the
            // SplashScreen (which mounts AFTER the StartupAuthGate PIN check resolves,
            // i.e. potentially after these fire) can seed its phase text on mount
            // instead of missing early events.
            const emitProgress = (pct: number, phase: string) => {
                (window as Window & { __wcStartupProgress?: { pct: number; phase: string } }).__wcStartupProgress = { pct, phase };
                window.dispatchEvent(new CustomEvent('startup-progress', { detail: { pct, phase } }));
            };

            emitProgress(10, 'reading configuration');
            const cached = await runStartupJob({
                id: 'settings-cache', priority: 'critical', cost: 'light', timeoutMs: 1_500,
                run: (signal) => initSettings(false, undefined, signal),
            });
            if (cancelled) return;
            if (cached.outcome !== 'completed' || !cached.value) {
                setStartupDataState('stale');
                return;
            }

            // Settings are the immediate, persisted source of truth. Let the
            // user enter the shell now; slow PowerShell/native probes refresh
            // it progressively in the background instead of holding splash.
            emitProgress(70, 'showing saved settings');
            reportStartupPhase('settings_cache_hydrated');
            setStartupComplete(true);
            setStartupDataState('refreshing');
            void runStartupJob({
                id: 'startup-status', priority: 'background', cost: 'expensive', timeoutMs: 20_000,
                run: (signal) => initializeApp(signal),
            }).then((result) => {
                if (!cancelled) setStartupDataState(result.outcome === 'completed' && result.value === true ? 'ready' : 'stale');
            });
            void runStartupJob({
                id: 'system-probe', priority: 'background', cost: 'expensive', timeoutMs: 30_000,
                run: (signal) => systemProbeStoreRef.current.refresh(
                    () => initSettings(true, cached.value ?? undefined, signal), signal,
                ),
            }).then((result) => {
                if (!cancelled && result.outcome === 'completed') reportStartupPhase('fresh_system_probe_complete');
            });

            emitProgress(95, 'refreshing system data');

            const dependencyStep = getStartupStaggerStep("dependencies");
            scheduleAfter(dependencyStep.delayMs, () => {
                void refreshBranding();
                if (canRunStartupJob('dependencies', startupEligibilityRef.current)) {
                    void runStartupJob({ id: 'dependencies', priority: 'background', cost: 'expensive', timeoutMs: 20_000, run: () => refreshDependencies(true) });
                }
            });

            // Pre-warm mesh status in background so the Private Mesh panel
            // has a cached value ready when the user first opens it.
            const meshStep = getStartupStaggerStep("mesh");
            scheduleAfter(meshStep.delayMs, () => {
                if (canRunStartupJob('mesh-status', startupEligibilityRef.current)) {
                    void runStartupJob({ id: 'mesh-status', priority: 'background', cost: 'light', timeoutMs: 8_000, run: () => refreshMesh(true) });
                }
            });

            // Lazy hardware refresh: re-run Get-SystemInfo to refresh the cached
            // CPU/GPU/RAM/disk fields in current.device.* without blocking load.
            // Warm cache (normal launch): defer 15s — the UI already shows last
            // session's values. Cold cache (first run): fetch in 1.5s so the
            // hardware labels fill in promptly instead of sitting blank.
            const sysInfoDelay = deviceCacheWarm ? 15_000 : 1_500;
            scheduleAfter(sysInfoDelay, () => {
                void getSystemInfo().then(async (res) => {
                    if (cancelled || !res.success || !res.data) return;
                    const incoming = res.data as any;
                    setSystemInfo(prev => ({
                        ...(prev ?? {}),
                        ...incoming,
                        // Preserve live Rust metrics — don't overwrite with PS stale values.
                        cpuUsage: prev?.cpuUsage ?? incoming.cpuUsage ?? 0,
                        cpuTemp: prev?.cpuTemp ?? incoming.cpuTemp ?? 0,
                        ramUsage: prev?.ramUsage ?? incoming.ramUsage ?? 0,
                        disks: mergeDiskHealth(incoming.disks ?? [], smartHealthCache),
                    } as any));
                    await persistProbeToSettings({ systemInfo: incoming });
                }).catch(() => {});
            });

            // Do not block startup completion on winget inventory. It is the
            // heaviest startup probe, so wait for an idle window after the
            // dashboard has rendered and lighter checks have had their turn.
            const inventoryStep = getStartupStaggerStep("inventory");
            scheduleWhenIdle(inventoryStep.delayMs, () => {
                if (canRunStartupJob('app-inventory', { ...startupEligibilityRef.current, hasIdleWindow: true })) {
                    void runStartupJob({ id: 'app-inventory', priority: 'idle', cost: 'expensive', timeoutMs: APP_INVENTORY_SOFT_TIMEOUT_MS, run: () => runAppInventoryScan(true) })
                        .then((result) => {
                            // A frontend timeout cannot kill winget/native work.
                            // Do not claim idle while that underlying operation drains.
                            if (result.outcome === 'completed' || result.outcome === 'failed') {
                                reportStartupPhase('background_idle');
                            }
                        });
                } else {
                    reportStartupPhase('background_idle');
                }
            });
        };

        runStartupSequence().catch((err) => {
            console.error('Startup sequence failed:', err);
        });

        return () => {
            cancelled = true;
            timers.forEach((timer) => clearTimeout(timer));
            const win = window as Window & { cancelIdleCallback?: (handle: number) => void };
            if (typeof win.cancelIdleCallback === 'function') {
                idleCallbacks.forEach((handle) => win.cancelIdleCallback!(handle));
            }
        };
    }, [initializeApp, initSettings, refreshMesh, runAppInventoryScan, refreshDependencies, refreshBranding, getSystemInfo, mergeDiskHealth, persistProbeToSettings, runStartupJob]);

    // Periodic app inventory refresh — keeps the radar's "APP UPDATES PENDING"
    // count and the Update All Apps button current without requiring the user
    // to manually refresh the Apps panel. 30 min mirrors the original module
    // contract ("startup + 60 min interval") halved so newly published winget
    // updates surface within ~30 min of the publisher cutting them.
    useEffect(() => {
        const interval = setInterval(() => {
            void runAppInventoryScan(true);
        }, 30 * 60 * 1000);
        return () => clearInterval(interval);
    }, [runAppInventoryScan]);

    const ctx = useMemo(() => ({
        systemInfo,
        meshInstalled,
        meshStatus,
        defenderStatus,
        updateStatus,
        networkBlocklistStatus,
        networkDnsStatus,
        encryptionStatus,
        productivityStatus,
        // KT: in decoy mode return inert fake data so panels show a plausible PC, not a blank install
        appInventory: authMode === "decoy" ? DECOY_INVENTORY : appInventory,
        appSettings: authMode === "decoy" ? DECOY_APP_SETTINGS : appSettings,
        patchAppSettings,
        refreshSettings,
        dependencyStatus,
        depCacheAge,
        forceRefreshDeps,
        loading,
        startupComplete,
        startupDataState,
        refreshAll,
        refreshSystem,
        refreshHardening,
        refreshPrivacy,
        refreshDashboard,
        refreshNetwork,
        refreshMesh,
        markMeshInstalled,
        refreshBranding,
        refreshVault,
        refreshProductivity,
        runAppInventoryScan,
        refreshDependencies,
        refreshDriveHealth,
        runStartupJob,
    }), [
        systemInfo,
        meshInstalled,
        meshStatus,
        defenderStatus,
        updateStatus,
        networkBlocklistStatus,
        networkDnsStatus,
        encryptionStatus,
        productivityStatus,
        appInventory,
        authMode,
        appSettings,
        patchAppSettings,
        refreshSettings,
        dependencyStatus,
        depCacheAge,
        forceRefreshDeps,
        loading,
        startupComplete,
        startupDataState,
        refreshAll,
        refreshSystem,
        refreshHardening,
        refreshPrivacy,
        refreshDashboard,
        refreshNetwork,
        refreshMesh,
        markMeshInstalled,
        refreshBranding,
        refreshVault,
        refreshProductivity,
        runAppInventoryScan,
        refreshDependencies,
        refreshDriveHealth,
        runStartupJob,
    ]);

    return (
        <AppContext.Provider value={ctx}>
            {children}
        </AppContext.Provider>
    );
};

export const useAppState = () => {
    const context = useContext(AppContext);
    if (!context) {
        throw new Error('useAppState must be used within an AppProvider');
    }
    return context;
};
