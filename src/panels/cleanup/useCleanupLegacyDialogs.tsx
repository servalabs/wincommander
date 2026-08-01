// src/panels/cleanup/useCleanupLegacyDialogs.tsx
// The older bespoke per-category viewer/clear dialogs. Card clicks in the
// main grid now route through the shared TraceDetailDialog instead — these
// remain for command parity while the panel is retired in slices (see the
// comment above `detailOpenerMap` in index.tsx). Extracted verbatim from
// src/panels/cleanup/index.tsx — pure move, no behavior change.
//
// This is a hook (not a component): it owns ~28 legacy dialogs' worth of
// state and returns `{ openers, dialogs }` so index.tsx can wire card-click
// openers into `detailOpenerMap` and render `dialogs` inline.
import { Dialog, Button, Tooltip, Icon } from "@/components/ui/bp";
import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState } from "react";
import { staggerDelay } from "../../components/shared/AnimatedList";
import { DURATION_S } from "../../components/shared/motion";
import useBackend, { UsbHistoryItem, DnsCacheEntry, ExecutionCacheEntry, ShellBagEntry, ProcessIntelligenceEntry, WlanProfile, BluetoothDevice, NetworkDrive, EventLogEntry, SRUMEntry, PSHistoryEntry, RecentFileEntry, RDPHistoryEntry, JumpListEntry, BrowserFootprintBrowser, PrefetchEntry, ShadowCopy, NTFSJournal } from "../../hooks/useBackend";
import { showSuccess, showError } from "../../utils/toast";
import UniversalCallout from "../../components/shared/UniversalCallout";
import { type CardData, updateCacheEntry } from "./useCleanupScan";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";

const formatExecPath = (rawPath: string) => {
    let p = rawPath || "";
    if (p.endsWith(".ApplicationCompany")) p = p.replace(/\.ApplicationCompany$/i, "");
    if (p.endsWith(".FriendlyAppName")) p = p.replace(/\.FriendlyAppName$/i, "");

    const lower = p.toLowerCase();
    let prefixIcon = null;
    let prefixText = "";
    let restOfPath = p;

    if (lower.startsWith("c:\\program files\\")) {
        prefixIcon = "folder-shared"; prefixText = "PF";
        restOfPath = p.substring("c:\\program files\\".length);
    } else if (lower.startsWith("c:\\program files (x86)\\")) {
        prefixIcon = "folder-shared"; prefixText = "PF32";
        restOfPath = p.substring("c:\\program files (x86)\\".length);
    } else if (lower.startsWith("c:\\programdata\\")) {
        prefixIcon = "database"; prefixText = "PD";
        restOfPath = p.substring("c:\\programdata\\".length);
    } else if (lower.startsWith("c:\\windows\\")) {
        prefixIcon = "desktop"; prefixText = "WIN";
        restOfPath = p.substring("c:\\windows\\".length);
    } else if (lower.match(/^c:\\users\\[^\\]+\\appdata\\local\\/i)) {
        prefixIcon = "user"; prefixText = "LOCAL";
        restOfPath = p.replace(/^c:\\users\\[^\\]+\\appdata\\local\\/i, "");
    } else if (lower.match(/^c:\\users\\[^\\]+\\appdata\\roaming\\/i)) {
        prefixIcon = "user"; prefixText = "ROAMING";
        restOfPath = p.replace(/^c:\\users\\[^\\]+\\appdata\\roaming\\/i, "");
    } else if (lower.match(/^c:\\users\\[^\\]+\\/i)) {
        prefixIcon = "user"; prefixText = "USER";
        restOfPath = p.replace(/^c:\\users\\[^\\]+\\/i, "");
    }

    if (prefixIcon) {
        return (
            <div style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                <div title={p} style={{ display: "flex", alignItems: "center", gap: "4px", background: "var(--color-bg-tertiary)", padding: "2px 6px", borderRadius: "4px", color: "var(--color-text-muted)", cursor: "help" }}>
                    <Icon icon={prefixIcon as any} size={10} />
                    <span style={{ fontSize: "9px", fontWeight: "bold" }}>{prefixText}</span>
                </div>
                <span style={{ wordBreak: "break-all" }}>{restOfPath}</span>
            </div>
        );
    }
    return <span>{p}</span>;
}

const formatSizeKB = (sizeKB: number) => {
    if (sizeKB >= 1024 * 1024) return `${(sizeKB / 1024 / 1024).toFixed(2)} GB`;
    if (sizeKB >= 1024) return `${(sizeKB / 1024).toFixed(1)} MB`;
    return `${Number(sizeKB || 0).toFixed(sizeKB >= 100 ? 0 : 1)} KB`;
};

export function useCleanupLegacyDialogs(cardDataMap: Record<string, CardData>) {
    const requestConfirm = useAppConfirm();
    const [localLoadingMap, setLocalLoadingMap] = useState<Record<string, boolean>>({});

    const [usbDialogOpen, setUsbDialogOpen] = useState(false);
    const [dnsDialogOpen, setDnsDialogOpen] = useState(false);
    const [execDialogOpen, setExecDialogOpen] = useState(false);
    const [wlanDialogOpen, setWlanDialogOpen] = useState(false);
    const [btDialogOpen, setBtDialogOpen] = useState(false);
    const [netDialogOpen, setNetDialogOpen] = useState(false);
    const [shellBagsDialogOpen, setShellBagsDialogOpen] = useState(false);
    const [processIntelDialogOpen, setProcessIntelDialogOpen] = useState(false);
    const [eventLogDialogOpen, setEventLogDialogOpen] = useState(false);
    const [srumDialogOpen, setSrumDialogOpen] = useState(false);
    const [psHistoryDialogOpen, setPsHistoryDialogOpen] = useState(false);
    const [usbHistory, setUsbHistory] = useState<UsbHistoryItem[]>([]);
    const [dnsCache, setDnsCache] = useState<DnsCacheEntry[]>([]);
    const [execCache, setExecCache] = useState<ExecutionCacheEntry[]>([]);
    const [wlanProfiles, setWlanProfiles] = useState<WlanProfile[]>([]);
    const [btDevices, setBtDevices] = useState<BluetoothDevice[]>([]);
    const [netDrives, setNetDrives] = useState<NetworkDrive[]>([]);
    const [shellBags, setShellBags] = useState<ShellBagEntry[]>([]);
    const [processIntel, setProcessIntel] = useState<ProcessIntelligenceEntry[]>([]);
    const [eventLogs, setEventLogs] = useState<EventLogEntry[]>([]);
    const [srumEntries, setSrumEntries] = useState<SRUMEntry[]>([]);
    const [psHistory, setPsHistory] = useState<PSHistoryEntry[]>([]);
    const [psHistoryPath, setPsHistoryPath] = useState<string | null>(null);
    const [psHistoryFileTotal, setPsHistoryFileTotal] = useState<number>(0);

    const [recentFilesDialogOpen, setRecentFilesDialogOpen] = useState(false);
    const [rdpHistoryDialogOpen, setRdpHistoryDialogOpen] = useState(false);
    const [jumpListsDialogOpen, setJumpListsDialogOpen] = useState(false);
    const [browserFootprintsDialogOpen, setBrowserFootprintsDialogOpen] = useState(false);
    const [prefetchDialogOpen, setPrefetchDialogOpen] = useState(false);
    const [shadowCopiesDialogOpen, setShadowCopiesDialogOpen] = useState(false);
    const [ntfsJournalsDialogOpen, setNtfsJournalsDialogOpen] = useState(false);
    const [recentFiles, setRecentFiles] = useState<RecentFileEntry[]>([]);
    const [rdpHistory, setRdpHistory] = useState<RDPHistoryEntry[]>([]);
    const [jumpLists, setJumpLists] = useState<JumpListEntry[]>([]);
    const [browserFootprints, setBrowserFootprints] = useState<BrowserFootprintBrowser[]>([]);
    const [prefetchFiles, setPrefetchFiles] = useState<PrefetchEntry[]>([]);
    const [prefetchAccessDenied, setPrefetchAccessDenied] = useState(false);
    const [prefetcherEnabled, setPrefetcherEnabled] = useState<number>(3);
    const [shadowCopies, setShadowCopies] = useState<ShadowCopy[]>([]);
    const [vssRunning, setVssRunning] = useState(true);
    const [ntfsJournals, setNtfsJournals] = useState<NTFSJournal[]>([]);


    const [amcacheDialogOpen, setAmcacheDialogOpen] = useState(false);
    const [ntUserTracesDialogOpen, setNtUserTracesDialogOpen] = useState(false);
    const [notepadStateDialogOpen, setNotepadStateDialogOpen] = useState(false);
    const [pcaInfoDialogOpen, setPcaInfoDialogOpen] = useState(false);
    const [crashDumpsDialogOpen, setCrashDumpsDialogOpen] = useState(false);
    const [recallDbDialogOpen, setRecallDbDialogOpen] = useState(false);
    const [amcacheData, setAmcacheData] = useState<{ entries: Array<{ category: string; count: number; sample: Array<{ id: string; name: string; path: string }> }>; hveFileSizeMb: number; hveFileExists: boolean; total: number } | null>(null);
    const [ntUserTracesData, setNtUserTracesData] = useState<{ sections: Array<{ name: string; count: number; entries: Array<{ key: string; value: string }> }>; total: number } | null>(null);
    const [notepadStateData, setNotepadStateData] = useState<{ files: Array<{ name: string; sizeKB: number; modified: string }>; total: number; packageFound: boolean; totalSizeKB: number } | null>(null);
    const [pcaInfoData, setPcaInfoData] = useState<{ files: Array<{ name: string; sizeKB: number; modified: string; type: string }>; total: number; totalSizeMB: number; pcaSvcState: string } | null>(null);
    const [crashDumpsData, setCrashDumpsData] = useState<{ dumps: Array<{ source: string; name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number } | null>(null);
    const [recallDbData, setRecallDbData] = useState<{ databases: Array<{ source: string; name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number } | null>(null);
    const [walFilesDialogOpen, setWalFilesDialogOpen] = useState(false);
    const [walFilesData, setWalFilesData] = useState<{ files: Array<{ name: string; sizeKB: number; dir: string; modified: string }>; total: number; totalSizeMB: number } | null>(null);
    const [searchIndexDialogOpen, setSearchIndexDialogOpen] = useState(false);
    const [searchIndexData, setSearchIndexData] = useState<{ files: Array<{ label: string; name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number; wsearchState: string } | null>(null);
    const [printSpoolerDialogOpen, setPrintSpoolerDialogOpen] = useState(false);
    const [printSpoolerData, setPrintSpoolerData] = useState<{ files: Array<{ source: string; name: string; sizeKB: number; modified: string }>; total: number; totalSizeMB: number; spoolerState: string } | null>(null);

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
        clearShadowCopies,
        getNTFSJournals,
        clearRecentFiles,
        clearRDPHistory,
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

        clearSearchIndex,
        clearPrintSpooler,
        invokeSQLiteWALKiller,
        clearRecallDatabase,
        getAmcacheEntries,
        getNTUserTraces,
        getNotepadStateFiles,
        getPCAInfo,
        getCrashDumpList,
        getSQLiteWALList,
        getRecallDatabaseInfo,
        getSearchIndexInfo,
        getPrintSpoolerInfo,
    } = useBackend();

    const runLocalOnly = async (key: string, fn: () => Promise<any>) => {
        setLocalLoadingMap(prev => ({ ...prev, [key]: true }));
        try {
            if (fn) await fn();
        } finally {
            setLocalLoadingMap(prev => ({ ...prev, [key]: false }));
        }
    };

    const shortenPath = (path: string | null | undefined, maxSegments = 3): string => {
        if (!path || !path.trim()) return "—";
        const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
        if (parts.length <= maxSegments) return path;
        return "…\\" + parts.slice(-maxSegments).join("\\");
    };
    const handleUsbHistory = async () => {
        const cached = cardDataMap['usbHistory']?.raw;
        if (cached?.devices) { setUsbHistory(cached.devices); setUsbDialogOpen(true); return; }
        await runLocalOnly('usbHistory', async () => {
            const res = await getUsbHistory();
            if (res.success && res.data?.devices) {
                setUsbHistory(res.data.devices);
                setUsbDialogOpen(true);
            } else {
                showError(res.error || "Failed to load USB history.");
            }
        });
    };

    const handleClearUsbHistory = async () => {
        await runLocalOnly('usbClear', async () => {
            const res = await clearUsbHistory();
            if (res.success) {
                setUsbHistory([]);
                updateCacheEntry('usbHistory', { count: 0, items: [], loading: false, clearing: false });
            } else {
                showError(res.error || "Failed to clear USB history.");
            }
        });
    };

    const handleShellBags = async () => {
        const cached = cardDataMap['shellBags']?.raw;
        if (cached?.entries) {
            setShellBags(Array.isArray(cached.entries) ? cached.entries : []);
            setShellBagsDialogOpen(true);
            return;
        }
        setShellBagsDialogOpen(true);
        setShellBags([]);
        await runLocalOnly('shellBags', async () => {
            const res = await getShellBags();
            if (res.success) {
                const entries = Array.isArray(res.data?.entries) ? res.data.entries : (res.data && typeof res.data === 'object' && 'entries' in res.data) ? (res.data as { entries?: ShellBagEntry[] }).entries ?? [] : [];
                setShellBags(entries);
            } else {
                showError(res.error || "Failed to load ShellBags.");
            }
        });
    };

    const handleClearShellBags = async () => {
        await runLocalOnly('shellBagsClear', async () => {
            const res = await clearShellBags();
            if (res.success) {
                setShellBags([]);
                updateCacheEntry('shellBags', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("ShellBags cleared.");
            } else {
                showError(res.error || "Failed to clear ShellBags.");
            }
        });
    };

    const handleProcessIntelligence = async () => {
        const cached = cardDataMap['processIntel']?.raw;
        if (cached?.processes) { setProcessIntel(Array.isArray(cached.processes) ? cached.processes : []); setProcessIntelDialogOpen(true); return; }
        setProcessIntelDialogOpen(true);
        setProcessIntel([]);
        await runLocalOnly('processIntel', async () => {
            const res = await getProcessIntelligence();
            if (res.success && res.data?.processes) {
                setProcessIntel(Array.isArray(res.data.processes) ? res.data.processes : []);
            } else {
                showError(res.error || "Failed to load process intelligence.");
            }
        });
    };

    const handleDnsCache = async () => {
        const cached = cardDataMap['dnsCache']?.raw;
        if (cached?.entries) { setDnsCache(cached.entries); setDnsDialogOpen(true); return; }
        await runLocalOnly('dnsCache', async () => {
            const res = await getDnsCacheEntries();
            if (res.success && res.data?.entries) {
                setDnsCache(res.data.entries);
                setDnsDialogOpen(true);
            } else {
                showError(res.error || "Failed to load DNS cache.");
            }
        });
    };

    const handleExecutionCache = async () => {
        const cached = cardDataMap['execCache']?.raw;
        if (cached?.entries) { setExecCache(Array.isArray(cached.entries) ? cached.entries : []); setExecDialogOpen(true); return; }
        setExecDialogOpen(true);
        setExecCache([]);
        await runLocalOnly('execCache', async () => {
            const res = await getExecutionCache();
            if (res.success) {
                const entries = Array.isArray(res.data?.entries) ? res.data.entries : (res.data && typeof res.data === 'object' && 'entries' in res.data) ? (res.data as { entries?: ExecutionCacheEntry[] }).entries ?? [] : [];
                setExecCache(entries);
            } else {
                showError(res.error || "Failed to load execution cache.");
            }
        });
    };

    const handleClearExecutionCache = async () => {
        await runLocalOnly('execClear', async () => {
            const res = await clearExecutionCache();
            if (res.success) {
                setExecCache([]);
                updateCacheEntry('execCache', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("Execution cache cleared successfully.");
            } else {
                showError(res.error || "Failed to clear execution cache.");
            }
        });
    };

    const handleWlanProfiles = async () => {
        const cached = cardDataMap['wlanProfiles']?.raw;
        if (cached?.profiles) { setWlanProfiles(cached.profiles); setWlanDialogOpen(true); return; }
        await runLocalOnly('wlanProfiles', async () => {
            const res = await getWlanProfiles();
            if (res.success && res.data?.profiles) {
                setWlanProfiles(res.data.profiles);
                setWlanDialogOpen(true);
            } else {
                showError(res.error || "Failed to load WLAN profiles.");
            }
        });
    };

    const handleRemoveWlanProfile = async (name: string) => {
        await runLocalOnly('wlanRemove', async () => {
            const res = await removeWlanProfile(name);
            if (res.success) {
                setWlanProfiles(prev => prev.filter(p => p.name !== name));
            } else {
                showError(res.error || "Failed to remove WLAN profile.");
            }
        });
    };

    const handleEraseAllWlan = async () => {
        const accepted = await requestConfirm({
            title: "Forget all Wi-Fi networks?",
            description: "Every saved Wi-Fi profile will be removed and the current wireless connection may be lost.",
            confirmLabel: "Forget all networks",
        });
        if (!accepted) return;
        await runLocalOnly('wlanErase', async () => {
            const res = await removeWlanProfile("");
            if (res.success) {
                setWlanProfiles([]);
                updateCacheEntry('wlanProfiles', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("All Wi-Fi profiles cleared.");
            } else {
                showError(res.error || "Failed to clear Wi-Fi profiles.");
            }
        });
    };

    const handleBtDevices = async () => {
        const cached = cardDataMap['btDevices']?.raw;
        if (cached?.devices) { setBtDevices(Array.isArray(cached.devices) ? cached.devices as BluetoothDevice[] : []); setBtDialogOpen(true); return; }
        await runLocalOnly('btDevices', async () => {
            const res = await getBluetoothDevices();
            // @ts-ignore - Backend now returns debugInfo and message on failure/empty
            const { devices, debugInfo, message } = res.data || {};

            if (res.success && devices && (Array.isArray(devices) ? devices.length > 0 : true)) {
                const devs = Array.isArray(devices) ? devices : [devices];
                setBtDevices(devs as BluetoothDevice[]);
                setBtDialogOpen(true);
            } else {
                setBtDevices([]);
                setBtDialogOpen(true);

                const errString = res.success
                    ? (message || debugInfo ? `${message || ""} ${debugInfo ? `\nLogs: ${debugInfo}` : ""}` : null)
                    : res.error;

                if (errString) {
                    showError(`Bluetooth Diag: ${errString}`);
                }
            }
        });
    };

    const handleClearBtHistory = async () => {
        await runLocalOnly('btClear', async () => {
            const res = await clearBluetoothHistory();
            if (res.success) {
                setBtDevices([]);
                updateCacheEntry('btDevices', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("Bluetooth history cleared.");
            } else {
                showError(res.error || "Failed to clear Bluetooth history.");
            }
        });
    };

    const handleNetDrives = async () => {
        const cached = cardDataMap['netDrives']?.raw;
        if (cached?.drives) { setNetDrives(Array.isArray(cached.drives) ? cached.drives as NetworkDrive[] : []); setNetDialogOpen(true); return; }
        await runLocalOnly('netDrives', async () => {
            const res = await getNetworkDrives();
            if (res.success && res.data?.drives) {
                const drives = Array.isArray(res.data.drives) ? res.data.drives : [res.data.drives].filter(Boolean);
                setNetDrives(drives as NetworkDrive[]);
                setNetDialogOpen(true);
            } else {
                showError(res.error || "Failed to load network drives.");
            }
        });
    };

    const handleClearNetDrives = async () => {
        await runLocalOnly('netDriveClear', async () => {
            const res = await clearNetworkDrives();
            if (res.success) {
                setNetDrives([]);
                updateCacheEntry('netDrives', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("Network drives disconnected and history cleared.");
            } else {
                showError(res.error || "Failed to clear network drives.");
            }
        });
    };

    // ── New Cleanup Handlers ─────────────────────────────────────────────

    const handleRecentFiles = async () => {
        const cached = cardDataMap['recentFiles']?.raw;
        if (cached?.entries) { setRecentFiles(Array.isArray(cached.entries) ? cached.entries : []); setRecentFilesDialogOpen(true); return; }
        setRecentFilesDialogOpen(true);
        setRecentFiles([]);
        await runLocalOnly('recentFiles', async () => {
            const res = await getRecentFiles();
            if (res.success && res.data?.entries) {
                setRecentFiles(Array.isArray(res.data.entries) ? res.data.entries : []);
            } else {
                showError(res.error || "Failed to load recent files.");
            }
        });
    };

    const handleClearRecentFiles = async () => {
        await runLocalOnly('recentFilesClear', async () => {
            const res = await clearRecentFiles();
            if (res.success) {
                setRecentFiles([]);
                updateCacheEntry('recentFiles', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("Recent files cleared.");
            } else {
                showError(res.error || "Failed to clear recent files.");
            }
        });
    };

    const handleRDPHistory = async () => {
        const cached = cardDataMap['rdpHistory']?.raw;
        if (cached?.entries) { setRdpHistory(Array.isArray(cached.entries) ? cached.entries : []); setRdpHistoryDialogOpen(true); return; }
        setRdpHistoryDialogOpen(true);
        setRdpHistory([]);
        await runLocalOnly('rdpHistory', async () => {
            const res = await getRDPHistory();
            if (res.success && res.data?.entries) {
                setRdpHistory(Array.isArray(res.data.entries) ? res.data.entries : []);
            } else {
                showError(res.error || "Failed to load RDP history.");
            }
        });
    };

    const handleClearRDPHistory = async () => {
        await runLocalOnly('rdpHistoryClear', async () => {
            const res = await clearRDPHistory();
            if (res.success) {
                setRdpHistory([]);
                updateCacheEntry('rdpHistory', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("RDP history cleared.");
            } else {
                showError(res.error || "Failed to clear RDP history.");
            }
        });
    };

    const handleJumpLists = async () => {
        const cached = cardDataMap['jumpLists']?.raw;
        if (cached?.entries) { setJumpLists(Array.isArray(cached.entries) ? cached.entries : []); setJumpListsDialogOpen(true); return; }
        setJumpListsDialogOpen(true);
        setJumpLists([]);
        await runLocalOnly('jumpLists', async () => {
            const res = await getJumpLists();
            if (res.success && res.data?.entries) {
                setJumpLists(Array.isArray(res.data.entries) ? res.data.entries : []);
            } else {
                showError(res.error || "Failed to load jump lists.");
            }
        });
    };

    const handleClearJumpLists = async () => {
        await runLocalOnly('jumpListsClear', async () => {
            const res = await clearJumpLists();
            if (res.success) {
                setJumpLists([]);
                updateCacheEntry('jumpLists', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("Jump lists cleared.");
            } else {
                showError(res.error || "Failed to clear jump lists.");
            }
        });
    };

    const handleBrowserFootprints = async () => {
        const cached = cardDataMap['browserFootprints']?.raw;
        if (cached?.browsers) { setBrowserFootprints(Array.isArray(cached.browsers) ? cached.browsers : []); setBrowserFootprintsDialogOpen(true); return; }
        setBrowserFootprintsDialogOpen(true);
        setBrowserFootprints([]);
        await runLocalOnly('browserFootprints', async () => {
            const res = await getBrowserFootprints();
            if (res.success && res.data?.browsers) {
                setBrowserFootprints(Array.isArray(res.data.browsers) ? res.data.browsers : []);
            } else {
                showError(res.error || "Failed to audit browser footprints.");
            }
        });
    };

    const browserFootprintCacheKey = (b: BrowserFootprintBrowser) =>
        `browserFootprintsClear:${b.browser || ""}|${b.profilePath || ""}`;

    const syncBrowserFootprintsCache = (next: BrowserFootprintBrowser[]) => {
        updateCacheEntry('browserFootprints', {
            count: next.length,
            items: next.map((b) => b.browser),
            loading: false,
            clearing: false,
            raw: { browsers: next, totalBrowsers: next.length },
        });
    };

    const handleClearBrowserFootprints = async () => {
        await runLocalOnly('browserFootprintsClear', async () => {
            const res = await clearBrowserFootprints();
            if (res.success) {
                setBrowserFootprints([]);
                syncBrowserFootprintsCache([]);
                showSuccess("Browser footprints cleared.");
            } else {
                showError(res.error || "Failed to clear browser footprints.");
            }
        });
    };

    const handleClearBrowserFootprint = async (target: BrowserFootprintBrowser) => {
        await runLocalOnly(browserFootprintCacheKey(target), async () => {
            const res = await clearBrowserFootprints(target.browser, target.profilePath);
            if (res.success) {
                const next = browserFootprints.filter(
                    (b) => b.browser !== target.browser || b.profilePath !== target.profilePath
                );
                setBrowserFootprints(next);
                syncBrowserFootprintsCache(next);
                showSuccess(`Browser cleared: ${target.browser}`);
            } else {
                showError(res.error || `Failed to clear browser ${target.browser}.`);
            }
        });
    };

    const handlePrefetchFiles = async () => {
        const cached = cardDataMap['prefetchFiles']?.raw;
        if (cached?.entries) { setPrefetchFiles(Array.isArray(cached.entries) ? cached.entries : []); setPrefetchAccessDenied(!!cached.accessDenied); setPrefetcherEnabled(cached.enablePrefetcher ?? 3); setPrefetchDialogOpen(true); return; }
        setPrefetchDialogOpen(true);
        setPrefetchFiles([]);
        setPrefetchAccessDenied(false);
        await runLocalOnly('prefetch', async () => {
            const res = await getPrefetchFiles();
            if (res.success && res.data) {
                setPrefetchFiles(Array.isArray(res.data.entries) ? res.data.entries : []);
                setPrefetchAccessDenied(!!res.data.accessDenied);
                setPrefetcherEnabled(res.data.enablePrefetcher ?? 3);
            } else {
                showError(res.error || "Failed to load prefetch files.");
            }
        });
    };

    const handleClearPrefetch = async () => {
        await runLocalOnly('prefetchClear', async () => {
            const res = await clearPrefetch();
            if (res.success) {
                setPrefetchFiles([]);
                updateCacheEntry('prefetchFiles', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("Prefetch files cleared.");
            } else {
                showError(res.error || "Failed to clear prefetch files.");
            }
        });
    };

    const handleShadowCopies = async () => {
        const cached = cardDataMap['shadowCopies']?.raw;
        if (cached?.copies) { setShadowCopies(Array.isArray(cached.copies) ? cached.copies : []); setVssRunning(cached.vssRunning ?? true); setShadowCopiesDialogOpen(true); return; }
        setShadowCopiesDialogOpen(true);
        setShadowCopies([]);
        await runLocalOnly('shadowCopies', async () => {
            const res = await getShadowCopies();
            if (res.success && res.data) {
                setShadowCopies(Array.isArray(res.data.copies) ? res.data.copies : []);
                setVssRunning(res.data.vssRunning ?? true);
            } else {
                showError(res.error || "Failed to list shadow copies.");
            }
        });
    };

    const handleClearShadowCopies = async () => {
        const accepted = await requestConfirm({
            title: "Delete all shadow copies?",
            description: "All Volume Shadow Service snapshots and their restore data will be deleted. This cannot be undone.",
            confirmLabel: "Delete snapshots",
        });
        if (!accepted) return;
        await runLocalOnly('shadowCopiesClear', async () => {
            const res = await clearShadowCopies();
            if (res.success) {
                setShadowCopies([]);
                updateCacheEntry('shadowCopies', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("All shadow copies deleted.");
            } else {
                showError(res.error || "Failed to delete shadow copies.");
            }
        });
    };

    const handleNTFSJournals = async () => {
        const cached = cardDataMap['ntfsJournals']?.raw;
        if (cached?.journals) { setNtfsJournals(Array.isArray(cached.journals) ? cached.journals : []); setNtfsJournalsDialogOpen(true); return; }
        setNtfsJournalsDialogOpen(true);
        setNtfsJournals([]);
        await runLocalOnly('ntfsJournals', async () => {
            const res = await getNTFSJournals();
            if (res.success && res.data?.journals) {
                setNtfsJournals(Array.isArray(res.data.journals) ? res.data.journals : []);
            } else {
                showError(res.error || "Failed to query NTFS journals.");
            }
        });
    };

    const handleClearNTFSJournals = async () => {
        await runLocalOnly('ntfsJournalsClear', async () => {
            const res = await clearNTFSJournals();
            if (res.success) {
                setNtfsJournals([]);
                updateCacheEntry('ntfsJournals', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("NTFS USN journals deleted.");
            } else {
                showError(res.error || "Failed to clear NTFS journals.");
            }
        });
    };

    const handleEventLogs = async () => {
        const cached = cardDataMap['eventLogs']?.raw;
        if (cached?.logs) { setEventLogs(Array.isArray(cached.logs) ? cached.logs : []); setEventLogDialogOpen(true); return; }
        setEventLogDialogOpen(true);
        setEventLogs([]);
        await runLocalOnly('eventLogView', async () => {
            const res = await getEventLogSummary();
            if (res.success && res.data?.logs) {
                setEventLogs(Array.isArray(res.data.logs) ? res.data.logs : []);
            } else {
                showError(res.error || "Failed to load event log summary.");
            }
        });
    };

    const handleClearEventLogsFromDialog = async () => {
        await runLocalOnly('eventLogClear', async () => {
            const res = await clearEventLogs();
            if (res.success) {
                setEventLogs([]);
                updateCacheEntry('eventLogs', { count: 0, items: [], loading: false, clearing: false });
                setEventLogDialogOpen(false);
                showSuccess("All event logs cleared.");
            } else {
                showError(res.error || "Failed to clear event logs.");
            }
        });
    };

    const handleSRUM = async () => {
        const cached = cardDataMap['srumData']?.raw;
        if (cached?.entries) { setSrumEntries(Array.isArray(cached.entries) ? cached.entries : []); setSrumDialogOpen(true); return; }
        setSrumDialogOpen(true);
        setSrumEntries([]);
        await runLocalOnly('srumView', async () => {
            const res = await getSRUMData();
            if (res.success && res.data?.entries) {
                setSrumEntries(Array.isArray(res.data.entries) ? res.data.entries : []);
            } else {
                showError(res.error || "Failed to load SRUM data.");
            }
        });
    };

    const handleClearSRUMFromDialog = async () => {
        await runLocalOnly('srumClear', async () => {
            const res = await clearSRUM();
            if (res.success) {
                setSrumEntries([]);
                updateCacheEntry('srumData', { count: 0, items: [], loading: false, clearing: false });
                setSrumDialogOpen(false);
                showSuccess("SRUM database cleared.");
            } else {
                showError(res.error || "Failed to clear SRUM.");
            }
        });
    };

    const handlePSHistory = async () => {
        const cached = cardDataMap['psHistory']?.raw;
        if (cached) { setPsHistory(Array.isArray(cached.entries) ? cached.entries : []); setPsHistoryPath(cached.historyPath ?? null); setPsHistoryFileTotal(cached.fileTotal ?? 0); setPsHistoryDialogOpen(true); return; }
        setPsHistoryDialogOpen(true);
        setPsHistory([]);
        await runLocalOnly('psHistView', async () => {
            const res = await getPSHistory();
            if (res.success && res.data) {
                setPsHistory(Array.isArray(res.data.entries) ? res.data.entries : []);
                setPsHistoryPath(res.data.historyPath ?? null);
                setPsHistoryFileTotal(res.data.fileTotal ?? 0);
            } else {
                showError(res.error || "Failed to load PowerShell history.");
            }
        });
    };

    const handleClearPSHistoryFromDialog = async () => {
        await runLocalOnly('psHistClear', async () => {
            const res = await clearPowerShellHistory();
            if (res.success) {
                setPsHistory([]);
                updateCacheEntry('psHistory', { count: 0, items: [], loading: false, clearing: false });
                setPsHistoryDialogOpen(false);
                showSuccess("Command history cleared.");
            } else {
                showError(res.error || "Failed to clear PowerShell history.");
            }
        });
    };


    // GROUP I-A: Advanced DFIR handlers
    const handleClearAmcache = async () => {
        const accepted = await requestConfirm({
            title: "Purge Amcache traces?",
            description: "Live Amcache registry keys will be cleared immediately and the .hve file will be scheduled for deletion during boot.",
            confirmLabel: "Purge Amcache",
        });
        if (!accepted) return;
        await runLocalOnly('amcache', async () => {
            const res = await clearAmcache();
            if (res.success && res.data) {
                const d = res.data;
                updateCacheEntry('amcache', { count: 0, items: [], loading: false, clearing: false });
                showSuccess(`Amcache cleared — ${d.clearedKeys} keys removed${d.bootPurgeScheduled ? '; .hve scheduled for boot deletion' : ''}.`);
            } else {
                showError(res.error || "Amcache purge failed.");
            }
        });
    };

    const handleClearNTUserTraces = async () => {
        await runLocalOnly('ntUserTraces', async () => {
            const res = await clearNTUserTraces();
            if (res.success && res.data) {
                updateCacheEntry('ntUserTraces', { count: 0, items: [], loading: false, clearing: false });
                showSuccess(`RunMRU/TypedPaths/OpenSaveMRU cleared — ${res.data.removedEntries} entries removed.`);
            } else {
                showError(res.error || "Activity traces clear failed.");
            }
        });
    };

    const handleClearNotepadState = async () => {
        await runLocalOnly('notepadState', async () => {
            const res = await clearNotepadState();
            if (res.success && res.data) {
                updateCacheEntry('notepadState', { count: 0, items: [], loading: false, clearing: false });
                showSuccess(`Notepad state purged — ${res.data.removedFiles} files removed.`);
            } else {
                showError(res.error || "Notepad state purge failed.");
            }
        });
    };

    const handleClearPCADatabase = async () => {
        await runLocalOnly('pcaDatabase', async () => {
            const res = await clearPCADatabase();
            if (res.success && res.data) {
                updateCacheEntry('pcaDatabase', { count: 0, items: [], loading: false, clearing: false });
                showSuccess(`PCA database cleared — ${res.data.removedFiles} files removed.`);
            } else {
                showError(res.error || "PCA database clear failed.");
            }
        });
    };

    const handleCrashDumpErase = async () => {
        await runLocalOnly('crashDumps', async () => {
            const res = await invokeCrashDumpErase();
            if (res.success && res.data) {
                updateCacheEntry('crashDumps', { count: 0, items: [], loading: false, clearing: false });
                showSuccess(`Crash dumps cleared — ${res.data.removedItems} items removed.`);
            } else {
                showError(res.error || "Crash dump clear failed.");
            }
        });
    };



    const handleClearSearchIndex = async () => {
        const accepted = await requestConfirm({
            title: "Clear the Windows Search index?",
            description: "Windows Search index databases and related index files will be removed. Search results will be incomplete or unavailable until Windows rebuilds them, which may take several minutes.",
            confirmLabel: "Clear index",
        });
        if (!accepted) return;
        await runLocalOnly('searchIndex', async () => {
            const res = await clearSearchIndex();
            if (res.success && res.data) {
                updateCacheEntry('searchIndex', { count: 0, items: [], loading: false, clearing: false });
                showSuccess(`Search index cleared — ${res.data.removedItems} files removed. Index is rebuilding.`);
            } else {
                showError(res.error || "Search index clear failed.");
            }
        });
    };

    const handleClearPrintSpooler = async () => {
        await runLocalOnly('printSpooler', async () => {
            const res = await clearPrintSpooler();
            if (res.success && res.data) {
                updateCacheEntry('printSpooler', { count: 0, items: [], loading: false, clearing: false });
                showSuccess(`Print spooler cleared — ${res.data.removedItems} files removed.`);
            } else {
                showError(res.error || "Print spooler clear failed.");
            }
        });
    };

    const handleSQLiteWALKiller = async () => {
        await runLocalOnly('walKiller', async () => {
            const res = await invokeSQLiteWALKiller();
            if (res.success && res.data) {
                updateCacheEntry('walFiles', { count: 0, items: [], loading: false, clearing: false });
                showSuccess(`Temp database cleanup: ${res.data.killedFiles} leftover files removed (${res.data.skippedLocked} skipped).`);
            } else {
                showError(res.error || "Temp database cleanup failed.");
            }
        });
    };

    const handleClearRecallDatabase = async () => {
        await runLocalOnly('recallDatabase', async () => {
            const res = await clearRecallDatabase();
            if (res.success && res.data) {
                updateCacheEntry('recallDb', { count: 0, items: [], loading: false, clearing: false });
                showSuccess(`Recall/Timeline databases purged — ${res.data.removedFiles} files removed.`);
            } else {
                showError(res.error || "Recall database purge failed.");
            }
        });
    };


    // GROUP I-A: Viewer handlers
    const handleViewAmcache = async () => {
        setAmcacheDialogOpen(true);
        setAmcacheData(null);
        await runLocalOnly('amcacheView', async () => {
            const res = await getAmcacheEntries();
            if (res.success && res.data) {
                setAmcacheData(res.data);
            } else {
                showError(res.error || "Failed to load Amcache entries.");
            }
        });
    };

    const handleViewNTUserTraces = async () => {
        setNtUserTracesDialogOpen(true);
        setNtUserTracesData(null);
        await runLocalOnly('ntUserTracesView', async () => {
            const res = await getNTUserTraces();
            if (res.success && res.data) {
                setNtUserTracesData(res.data);
            } else {
                showError(res.error || "Failed to load NTUSER traces.");
            }
        });
    };

    const handleViewNotepadState = async () => {
        setNotepadStateDialogOpen(true);
        setNotepadStateData(null);
        await runLocalOnly('notepadStateView', async () => {
            const res = await getNotepadStateFiles();
            if (res.success && res.data) {
                setNotepadStateData(res.data);
            } else {
                showError(res.error || "Failed to load Notepad state files.");
            }
        });
    };

    const handleViewPCAInfo = async () => {
        setPcaInfoDialogOpen(true);
        setPcaInfoData(null);
        await runLocalOnly('pcaInfoView', async () => {
            const res = await getPCAInfo();
            if (res.success && res.data) {
                setPcaInfoData(res.data);
            } else {
                showError(res.error || "Failed to load PCA info.");
            }
        });
    };

    const handleViewCrashDumps = async () => {
        setCrashDumpsDialogOpen(true);
        setCrashDumpsData(null);
        await runLocalOnly('crashDumpsView', async () => {
            const res = await getCrashDumpList();
            if (res.success && res.data) {
                setCrashDumpsData(res.data);
            } else {
                showError(res.error || "Failed to load crash dump list.");
            }
        });
    };

    const handleViewSearchIndex = async () => {
        setSearchIndexDialogOpen(true);
        setSearchIndexData(null);
        await runLocalOnly('searchIndexView', async () => {
            const res = await getSearchIndexInfo();
            if (res.success && res.data) {
                setSearchIndexData(res.data);
            } else {
                showError(res.error || "Failed to load search index info.");
            }
        });
    };

    const handleViewPrintSpooler = async () => {
        setPrintSpoolerDialogOpen(true);
        setPrintSpoolerData(null);
        await runLocalOnly('printSpoolerView', async () => {
            const res = await getPrintSpoolerInfo();
            if (res.success && res.data) {
                setPrintSpoolerData(res.data);
            } else {
                showError(res.error || "Failed to load print spooler info.");
            }
        });
    };

    const handleViewRecallDb = async () => {
        setRecallDbDialogOpen(true);
        setRecallDbData(null);
        await runLocalOnly('recallDbView', async () => {
            const res = await getRecallDatabaseInfo();
            if (res.success && res.data) {
                setRecallDbData(res.data);
            } else {
                showError(res.error || "Failed to load Recall/Timeline databases.");
            }
        });
    };

    const handleViewWALFiles = async () => {
        setWalFilesDialogOpen(true);
        setWalFilesData(null);
        await runLocalOnly('walFilesView', async () => {
            const res = await getSQLiteWALList();
            if (res.success && res.data) {
                setWalFilesData(res.data);
            } else {
                showError(res.error || "Failed to scan WAL files.");
            }
        });
    };

    // Close every open legacy dialog when the Pro activation celebration
    // fires so the confetti overlay isn't buried under one of these.
    useEffect(() => {
        const closeAll = () => {
            setUsbDialogOpen(false); setDnsDialogOpen(false); setExecDialogOpen(false);
            setWlanDialogOpen(false); setBtDialogOpen(false); setNetDialogOpen(false);
            setShellBagsDialogOpen(false); setProcessIntelDialogOpen(false);
            setEventLogDialogOpen(false); setSrumDialogOpen(false);
            setPsHistoryDialogOpen(false); setRecentFilesDialogOpen(false);
            setRdpHistoryDialogOpen(false); setJumpListsDialogOpen(false);
            setBrowserFootprintsDialogOpen(false); setPrefetchDialogOpen(false);
            setShadowCopiesDialogOpen(false); setNtfsJournalsDialogOpen(false);
            setAmcacheDialogOpen(false); setNtUserTracesDialogOpen(false);
            setNotepadStateDialogOpen(false); setPcaInfoDialogOpen(false);
            setCrashDumpsDialogOpen(false); setRecallDbDialogOpen(false);
            setWalFilesDialogOpen(false); setSearchIndexDialogOpen(false);
            setPrintSpoolerDialogOpen(false);
        };
        window.addEventListener("commander-dismiss-dialogs", closeAll);
        return () => window.removeEventListener("commander-dismiss-dialogs", closeAll);
    }, []);

    // Exposed so index.tsx's `detailOpenerMap` can route card clicks the same
    // way the original single-component version did (most entries are kept
    // referenced only, two — Wi-Fi and Browser — are actually invoked).
    const openers = {
        handleShellBags, handleUsbHistory, handleDnsCache, handleExecutionCache,
        handleWlanProfiles, handleBtDevices, handleNetDrives, handleProcessIntelligence,
        handleEventLogs, handleSRUM, handlePSHistory, handleRecentFiles, handleRDPHistory,
        getConnectivityHistory, handleJumpLists, handleBrowserFootprints, handlePrefetchFiles,
        handleShadowCopies, handleNTFSJournals, handleViewAmcache, handleViewNTUserTraces,
        handleViewNotepadState, handleViewPCAInfo, handleViewCrashDumps, handleViewSearchIndex,
        handleViewPrintSpooler, handleViewWALFiles, handleViewRecallDb,
    };

    return { openers, dialogs: (
        <>
            <Dialog
                isOpen={wlanDialogOpen}
                onClose={() => setWlanDialogOpen(false)}
                title="Wi-Fi Profiles"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <div className="mb-3 p-2 bg-blue-500/10 text-blue-400 text-xs rounded border border-blue-500/20">
                        Passwords are shown in cleartext. Use with caution.
                    </div>
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px" }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>SSID</th>
                                    <th>PASSWORD</th>
                                    <th style={{ width: 40 }}></th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {wlanProfiles.map((p, idx) => (
                                    <motion.tr
                                        key={p.name}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td className="font-semibold" style={{ color: 'var(--color-text-primary)' }}>{p.name}</td>
                                        <td className="font-mono text-xs opacity-80" style={{ color: 'var(--color-text-muted)' }}>{p.password || "<NO PASS>"}</td>
                                        <td>
                                            <Button
                                                icon="trash"
                                                small
                                                minimal
                                                intent="danger"
                                                onClick={() => handleRemoveWlanProfile(p.name)}
                                                loading={localLoadingMap["wlanRemove"]}
                                            />
                                        </td>
                                    </motion.tr>
                                ))}
                                {wlanProfiles.length === 0 && (
                                    <tr>
                                        <td colSpan={3} className="text-center opacity-50 py-4">No saved profiles found</td>
                                    </tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button
                        icon="cross"
                        text="CLOSE"
                        onClick={() => setWlanDialogOpen(false)}
                        minimal
                        className="modal-cancel-btn"
                    />
                    <Button
                        icon="trash"
                        text="CLEAR ALL PROFILES"
                        onClick={handleEraseAllWlan}
                        loading={localLoadingMap["wlanErase"]}
                        className="modal-primary-btn danger"
                        disabled={wlanProfiles.length === 0}
                    />
                </div>
            </Dialog>

            <Dialog
                isOpen={btDialogOpen}
                onClose={() => setBtDialogOpen(false)}
                title="Bluetooth History"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="HISTORY ONLY: This list shows historical connection traces. Clearing this will NOT disconnect or unpair currently active devices."
                        intent="primary"
                    />
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px" }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th style={{ width: "35%" }}>NAME</th>
                                    <th style={{ width: "40%" }}>DEVICE ID</th>
                                    <th style={{ width: "25%" }}>LAST SEEN</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {btDevices.map((d, idx) => (
                                    <motion.tr
                                        key={d.id}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td className="text-[var(--color-text-primary)] font-semibold">{d.name || "Unknown"}</td>
                                        <td className="font-mono text-[10px] text-gray-500" style={{ wordBreak: 'break-all' }}>{d.id}</td>
                                        <td className="text-gray-400 text-xs">
                                            {d.lastSeen && d.lastSeen !== "Unknown"
                                                ? new Date(d.lastSeen).toLocaleDateString('en-GB', { day: '2-digit', month: 'short', year: 'numeric' })
                                                : "Unknown"}
                                        </td>
                                    </motion.tr>
                                ))}
                                {btDevices.length === 0 && (
                                    <tr>
                                        <td colSpan={2} className="text-center opacity-50 py-4">No devices found</td>
                                    </tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button
                        icon="cross"
                        text="CLOSE"
                        onClick={() => setBtDialogOpen(false)}
                        minimal
                        className="modal-cancel-btn"
                    />
                    <Button
                        icon="trash"
                        text="CLEAR HISTORY"
                        onClick={handleClearBtHistory}
                        loading={localLoadingMap["btClear"]}
                        className="modal-primary-btn danger"
                    />
                </div>
            </Dialog>

            <Dialog
                isOpen={netDialogOpen}
                onClose={() => setNetDialogOpen(false)}
                title="Network Drives"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px" }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>DRIVE</th>
                                    <th>PATH</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {netDrives.map((d, idx) => (
                                    <motion.tr
                                        key={d.Name}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td className="font-bold text-[var(--color-text-primary)]">{d.Name}</td>
                                        <td className="font-mono text-xs text-gray-400">{d.DisplayRoot}</td>
                                    </motion.tr>
                                ))}
                                {netDrives.length === 0 && (
                                    <tr>
                                        <td colSpan={2} className="text-center opacity-50 py-4">No mapped drives found</td>
                                    </tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button
                        icon="cross"
                        text="CLOSE"
                        onClick={() => setNetDialogOpen(false)}
                        minimal
                        className="modal-cancel-btn"
                    />
                    <Button
                        icon="trash"
                        text="UNMAP ALL & CLEAR"
                        onClick={handleClearNetDrives}
                        loading={localLoadingMap["netDriveClear"]}
                        className="modal-primary-btn danger"
                    />
                </div>
            </Dialog>

            <Dialog
                isOpen={usbDialogOpen}
                onClose={() => setUsbDialogOpen(false)}
                title="USB Device History"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px" }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>DEVICE ID</th>
                                    <th>FRIENDLY NAME</th>
                                    <th>MANUFACTURER</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {usbHistory.map((d, idx) => (
                                    <motion.tr
                                        key={d.deviceId}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td className="mono-cell font-mono text-[11px]" style={{ maxWidth: 220, wordBreak: "break-all" }}>{d.deviceId}</td>
                                        <td>{d.friendlyName || "-"}</td>
                                        <td>{d.manufacturer || "-"}</td>
                                    </motion.tr>
                                ))}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button
                        icon="cross"
                        text="CLOSE"
                        onClick={() => setUsbDialogOpen(false)}
                        minimal
                        className="modal-cancel-btn"
                    />
                    <Button
                        icon="trash"
                        text="CLEAR HISTORY"
                        onClick={handleClearUsbHistory}
                        loading={localLoadingMap["usbClear"]}
                        className="modal-primary-btn danger"
                    />
                </div>
            </Dialog>



            {/* DNS Dialog */}
            <Dialog
                isOpen={dnsDialogOpen}
                onClose={() => setDnsDialogOpen(false)}
                title="DNS Cache"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <div className="flex items-center gap-3 mb-3 px-1 text-[11px] text-[var(--color-text-muted)]">
                        <span><strong className="text-[var(--color-text-primary)]">{dnsCache.length}</strong> entries cached</span>
                        <span className="opacity-50">·</span>
                        <span>Type = DNS record (A=IPv4, AAAA=IPv6, CNAME=alias)</span>
                        <span className="opacity-50">·</span>
                        <span>TTL in seconds</span>
                    </div>
                    <div style={{ maxHeight: 420, overflowY: "auto", paddingRight: "12px", border: "1px solid var(--color-border)", background: "var(--color-bg-tertiary)" }}>
                        <table className="dns-cache-table" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>NAME</th>
                                    <th style={{ width: 70 }}>TYPE</th>
                                    <th style={{ width: 90 }}>STATUS</th>
                                    <th style={{ width: 60, textAlign: "right" }}>TTL</th>
                                    <th style={{ width: 80 }}>SECTION</th>
                                    <th>DATA</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {dnsCache.map((d, idx) => {
                                    const anyD = d as any;
                                    const ttlNum = typeof d.ttl === "number" ? d.ttl : null;
                                    const ttlDisplay = ttlNum === null ? "—" : (ttlNum > 3600 ? `${Math.round(ttlNum / 3600)}h` : ttlNum > 60 ? `${Math.round(ttlNum / 60)}m` : `${ttlNum}s`);
                                    return (
                                        <motion.tr
                                            key={`${d.name}:${d.recordType ?? ''}:${d.data}`}
                                            initial={{ opacity: 0 }}
                                            animate={{ opacity: 1 }}
                                            exit={{ opacity: 0 }}
                                            transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                        >
                                            <td className="mono-cell font-mono text-[11px]" style={{ maxWidth: 220, wordBreak: "break-all" }} title={d.name}>{d.name}</td>
                                            <td className="font-mono text-[11px]" style={{ color: "var(--color-accent)", fontWeight: 600 }}>{d.recordType || "—"}</td>
                                            <td className="font-mono text-[11px]">{d.status || "—"}</td>
                                            <td className="text-right font-mono text-[11px] text-[var(--color-text-muted)]" title={ttlNum !== null ? `${ttlNum} seconds` : ''}>{ttlDisplay}</td>
                                            <td className="font-mono text-[10px] text-[var(--color-text-muted)] uppercase">{anyD.section || "—"}</td>
                                            <td className="font-mono text-[11px]" style={{ maxWidth: 320, wordBreak: "break-all" }}>{d.data || "—"}</td>
                                        </motion.tr>
                                    );
                                })}
                                {dnsCache.length === 0 && (
                                    <tr><td colSpan={6} className="text-center opacity-50 py-6">No DNS cache entries</td></tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button
                        icon="cross"
                        text="CLOSE"
                        onClick={() => setDnsDialogOpen(false)}
                        minimal
                        className="modal-cancel-btn"
                    />
                    <Button
                        icon="refresh"
                        text="FLUSH CACHE"
                        onClick={async () => {
                            await runLocalOnly('dnsFlush', flushDnsCache);
                            setDnsCache([]);
                            updateCacheEntry('dnsCache', { count: 0, items: [], loading: false, clearing: false });
                        }}
                        loading={localLoadingMap['dnsFlush']}
                        className="modal-primary-btn"
                    />
                </div>
            </Dialog>

            {/* Exec Dialog */}
            <Dialog
                isOpen={execDialogOpen}
                onClose={() => setExecDialogOpen(false)}
                title="Deep Execution Audit"
                className="mount-dialog priv-dialog"
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="Clearable execution traces: MuiCache, UserAssist, RecentApps, ShimCache, PCA. BAM is kernel-protected and not shown. Clear removes these and restarts Explorer."
                        intent="primary"
                    />
                    {execCache.length === 0 && (
                        <p className="text-xs mb-2" style={{ color: "var(--color-text-muted)" }}>
                            No clearable entries. Run as Administrator if empty when you expect data.
                        </p>
                    )}
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>PATH</th>
                                    <th>SOURCE</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {execCache.map((d, idx) => (
                                    <motion.tr
                                        key={`${d.path}:${d.source}`}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td className="mono-cell font-mono text-[11px]">{formatExecPath(d.path)}</td>
                                        <td>{d.source}</td>
                                    </motion.tr>
                                ))}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button
                        icon="cross"
                        text="CLOSE"
                        onClick={() => setExecDialogOpen(false)}
                        minimal
                        className="modal-cancel-btn"
                    />
                    <Button
                        icon="refresh"
                        text="REFRESH"
                        onClick={handleExecutionCache}
                        loading={localLoadingMap["execCache"]}
                        className="modal-primary-btn"
                    />
                    <Button
                        icon="trash"
                        text="CLEAR TRACES"
                        onClick={handleClearExecutionCache}
                        loading={localLoadingMap["execClear"]}
                        className="modal-primary-btn danger"
                    />
                </div>
            </Dialog>

            {/* ShellBags Dialog */}
            <Dialog
                isOpen={shellBagsDialogOpen}
                onClose={() => setShellBagsDialogOpen(false)}
                title="ShellBags Explorer"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="Cached folder access history from the registry. Hackers use these to reconstruct user activity."
                        intent="primary"
                    />
                    {(shellBags.length === 0 || (shellBags.length === 1 && shellBags[0]?.path === "ShellBag root")) && (
                        <p className="text-xs mb-2" style={{ color: "var(--color-text-muted)" }}>
                            No ShellBag entries found. Browse some folders in File Explorer to populate, or the cache may have been cleared.
                        </p>
                    )}
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>PATH / KEY</th>
                                    <th>LAST MODIFIED</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {shellBags.filter(d => d.path !== "ShellBag root").map((d, idx) => (
                                    <motion.tr
                                        key={d.path}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td className="mono-cell font-mono text-[11px]" style={{ maxWidth: 320, wordBreak: "break-all" }}>{d.path}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-secondary)" }}>
                                            {d.lastModified ? new Date(d.lastModified).toLocaleString() : "-"}
                                        </td>
                                    </motion.tr>
                                ))}
                                {(shellBags.length === 0 || (shellBags.length === 1 && shellBags[0]?.path === "ShellBag root")) && (
                                    <tr>
                                        <td colSpan={2} className="text-center opacity-50 py-4">No ShellBag entries found</td>
                                    </tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button
                        icon="cross"
                        text="CLOSE"
                        onClick={() => setShellBagsDialogOpen(false)}
                        minimal
                        className="modal-cancel-btn"
                    />
                    <Button
                        icon="trash"
                        text="CLEAR HISTORY"
                        onClick={handleClearShellBags}
                        loading={localLoadingMap["shellBagsClear"]}
                        className="modal-primary-btn danger"
                    />
                </div>
            </Dialog>

            {/* Process Review Dialog */}
            <Dialog
                isOpen={processIntelDialogOpen}
                onClose={() => setProcessIntelDialogOpen(false)}
                title="Process Review"
                className="mount-dialog priv-dialog process-intel-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <div className="mb-3 px-3 py-2 rounded border flex items-center gap-3 text-[10px]" style={{ background: "var(--color-bg-tertiary)", borderColor: "var(--color-border)", color: "var(--color-text-muted)" }}>
                        <span className="process-info-trigger" style={{ cursor: 'help', flexShrink: 0, display: 'flex', alignItems: 'center' }}>
                            <Icon icon="info-sign" size={11} />
                        </span>
                        <span className="process-info-text">
                            <span className="text-red-400 font-bold">High</span> = unsigned + elevated · <span className="text-yellow-400 font-bold">Medium</span> = unsigned · <span className="font-bold" style={{ color: 'var(--color-accent)' }}>System</span> = signed + SYSTEM · <span className="text-green-400 font-bold">Low</span> = signed
                        </span>
                    </div>
                    {(() => {
                        const isSigned = (p: ProcessIntelligenceEntry) => p.signed === "Valid";
                        const high = processIntel.filter(p => !isSigned(p) && p.elevated === "Yes");
                        const medium = processIntel.filter(p => !isSigned(p) && p.elevated !== "Yes");
                        const system = processIntel.filter(p => p.elevated === "Yes" && isSigned(p));
                        const low = processIntel.filter(p => isSigned(p) && p.elevated !== "Yes");
                        return (
                            <div className="flex gap-3 mb-3 text-[10px]">
                                <span className="px-2 py-1 rounded" style={{ background: "color-mix(in srgb, var(--color-danger) 20%, transparent)", color: "var(--color-danger)" }}>High: {high.length}</span>
                                <span className="px-2 py-1 rounded" style={{ background: "color-mix(in srgb, var(--color-warning) 20%, transparent)", color: "var(--color-warning)" }}>Medium: {medium.length}</span>
                                <span className="px-2 py-1 rounded" style={{ background: "color-mix(in srgb, var(--color-accent) 20%, transparent)", color: "var(--color-accent)" }}>System: {system.length}</span>
                                <span className="px-2 py-1 rounded" style={{ background: "var(--color-bg-tertiary)", color: "var(--color-text-muted)" }}>Low: {low.length}</span>
                                <span className="px-2 py-1 rounded" style={{ background: "var(--color-bg-tertiary)", color: "var(--color-text-muted)" }}>Total: {processIntel.length}</span>
                            </div>
                        );
                    })()}
                    <div style={{ maxHeight: 480, overflowY: "auto", paddingRight: "8px" }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>RISK</th>
                                    <th>NAME</th>
                                    <th>PID</th>
                                    <th>SIGNED</th>
                                    <th>ELEVATED</th>
                                    <th>PATH</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {[...processIntel]
                                    .sort((a, b) => {
                                        const isSigned = (x: ProcessIntelligenceEntry) => x.signed === "Valid";
                                        const risk = (p: ProcessIntelligenceEntry) =>
                                            !isSigned(p) && p.elevated === "Yes" ? 3 :
                                                !isSigned(p) ? 2 :
                                                    p.elevated === "Yes" ? 1 : 0;
                                        return risk(b) - risk(a);
                                    })
                                    .map((p, idx) => {
                                        const signedValid = p.signed === "Valid";
                                        const riskLevel = !signedValid && p.elevated === "Yes" ? "High" :
                                            !signedValid ? "Medium" :
                                                p.elevated === "Yes" ? "System" : "Low";
                                        const riskColor = riskLevel === "High" ? "var(--color-danger)" :
                                            riskLevel === "Medium" ? "var(--color-warning)" :
                                                riskLevel === "System" ? "var(--color-accent)" : "var(--color-text-muted)";
                                        const sigDisplay = signedValid ? "Valid" : p.signed;
                                        const sigTooltip = signedValid && p.signer ? p.signer : sigDisplay;
                                        return (
                                            <motion.tr
                                                key={`${p.pid}-${p.name}`}
                                                initial={{ opacity: 0 }}
                                                animate={{ opacity: 1 }}
                                                exit={{ opacity: 0 }}
                                                transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                            >
                                                <td><span className="font-semibold" style={{ color: riskColor }}>{riskLevel}</span></td>
                                                <td className="font-semibold" style={{ color: "var(--color-text-primary)" }}>{p.name}</td>
                                                <td className="font-mono text-xs">{p.pid}</td>
                                                <td>
                                                    <Tooltip content={sigTooltip} position="left">
                                                        <span className={signedValid ? "text-[var(--color-success)]" : "text-[var(--color-warning)]"} style={{ cursor: "help" }}>
                                                            {sigDisplay}
                                                        </span>
                                                    </Tooltip>
                                                </td>
                                                <td>{p.elevated}</td>
                                                <td className="mono-cell font-mono text-[10px] opacity-80" style={{ maxWidth: 220 }}>
                                                    <Tooltip content={p.path || "—"} position="left">
                                                        <span style={{ cursor: "help" }}>{shortenPath(p.path)}</span>
                                                    </Tooltip>
                                                </td>
                                            </motion.tr>
                                        );
                                    })}
                                {!localLoadingMap["processIntel"] && processIntel.length === 0 && (
                                    <tr>
                                        <td colSpan={6} className="text-center opacity-50 py-4">No processes found</td>
                                    </tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button
                        icon="cross"
                        text="CLOSE"
                        onClick={() => setProcessIntelDialogOpen(false)}
                        minimal
                        className="modal-cancel-btn"
                    />
                    <Button
                        icon="refresh"
                        text="REFRESH"
                        onClick={handleProcessIntelligence}
                        loading={localLoadingMap["processIntel"]}
                        className="modal-primary-btn"
                    />
                </div>
            </Dialog>

            {/* ── Event Log Summary Dialog ──────────────────────────── */}
            <Dialog
                isOpen={eventLogDialogOpen}
                onClose={() => setEventLogDialogOpen(false)}
                title="Event Log Summary"
                className="mount-dialog priv-dialog"
                isCloseButtonShown
                canEscapeKeyClose
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="Shows all non-empty event logs and their record counts. Clearing will permanently delete all entries across every log channel."
                        intent="warning"
                    />
                    {localLoadingMap['eventLogView'] && (
                        <div className="flex items-center justify-center py-6 text-[var(--color-text-muted)] text-sm gap-2">
                            <Icon icon="time" /> Loading event logs…
                        </div>
                    )}
                    {!localLoadingMap['eventLogView'] && eventLogs.length > 0 && (
                        <div className="mb-2 text-xs text-[var(--color-text-muted)]">
                            {eventLogs.length} non-empty log(s) · {eventLogs.reduce((s, l) => s + l.count, 0).toLocaleString()} total events
                        </div>
                    )}
                    <div style={{ maxHeight: 400, overflowY: "auto", paddingRight: "8px" }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>LOG NAME</th>
                                    <th style={{ width: 90, textAlign: "right" }}>EVENTS</th>
                                    <th style={{ width: 70, textAlign: "right" }}>SIZE</th>
                                    <th style={{ width: 140 }}>LAST WRITE</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {eventLogs.map((log, idx) => (
                                    <motion.tr
                                        key={log.name}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td className="font-mono text-[11px]" style={{ wordBreak: "break-all", color: "var(--color-text-primary)" }}>{log.name}</td>
                                        <td className="text-right font-mono text-[12px]" style={{ color: "var(--color-warning)" }}>{log.count.toLocaleString()}</td>
                                        <td className="text-right text-xs text-[var(--color-text-muted)]">{log.sizeMb != null ? `${log.sizeMb} MB` : "—"}</td>
                                        <td className="text-xs text-gray-400">
                                            {log.newest ? new Date(log.newest).toLocaleDateString('en-GB', { day: "2-digit", month: "short", year: "numeric" }) : "—"}
                                        </td>
                                    </motion.tr>
                                ))}
                                {!localLoadingMap['eventLogView'] && eventLogs.length === 0 && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">No events found</td></tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setEventLogDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button
                        icon="trash"
                        text="CLEAR ALL LOGS"
                        onClick={handleClearEventLogsFromDialog}
                        loading={localLoadingMap['eventLogClear']}
                        className="modal-primary-btn danger"
                        disabled={eventLogs.length === 0 && !localLoadingMap['eventLogView']}
                    />
                </div>
            </Dialog>

            {/* ── SRUM Data Dialog ──────────────────────────────────── */}
            <Dialog
                isOpen={srumDialogOpen}
                onClose={() => setSrumDialogOpen(false)}
                title="Resource Usage Snapshot"
                className="mount-dialog priv-dialog"
                isCloseButtonShown
                canEscapeKeyClose
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="System Resource Usage Monitor — top 50 running processes by CPU time. Clearing removes SRUDB.dat (requires stopping Diagnostic Policy Service)."
                        intent="primary"
                    />
                    {localLoadingMap['srumView'] && (
                        <div className="flex items-center justify-center py-6 text-[var(--color-text-muted)] text-sm gap-2">
                            <Icon icon="time" /> Loading SRUM snapshot…
                        </div>
                    )}
                    <div style={{ maxHeight: 400, overflowY: "auto", paddingRight: "8px" }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>PROCESS</th>
                                    <th style={{ width: 50 }}>PID</th>
                                    <th style={{ width: 85, textAlign: "right" }}>MEMORY</th>
                                    <th style={{ width: 60, textAlign: "right" }}>THREADS</th>
                                    <th>OWNER</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {srumEntries.map((e, idx) => (
                                    <motion.tr
                                        key={`${e.pid}-${e.name}`}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td>
                                            <Tooltip content={e.path || e.name} position="right">
                                                <span className="font-semibold" style={{ color: "var(--color-text-primary)", cursor: "help" }}>{e.name}</span>
                                            </Tooltip>
                                        </td>
                                        <td className="font-mono text-xs text-[var(--color-text-muted)]">{e.pid}</td>
                                        <td className="text-right font-mono text-[12px]" style={{ color: "var(--color-accent)" }}>
                                            {e.memoryKB >= 1024 ? `${Math.round(e.memoryKB / 1024)} MB` : `${e.memoryKB} KB`}
                                        </td>
                                        <td className="text-right text-xs text-[var(--color-text-muted)]">{e.threadCount}</td>
                                        <td className="text-xs text-gray-500 font-mono">{e.owner || "—"}</td>
                                    </motion.tr>
                                ))}
                                {!localLoadingMap['srumView'] && srumEntries.length === 0 && (
                                    <tr><td colSpan={5} className="text-center opacity-50 py-4">No data available</td></tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setSrumDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button
                        icon="trash"
                        text="CLEAR USAGE HISTORY"
                        onClick={handleClearSRUMFromDialog}
                        loading={localLoadingMap['srumClear']}
                        className="modal-primary-btn danger"
                    />
                </div>
            </Dialog>

            {/* ── PS History Dialog ─────────────────────────────────── */}
            <Dialog
                isOpen={psHistoryDialogOpen}
                onClose={() => setPsHistoryDialogOpen(false)}
                title="PowerShell Command History"
                className="mount-dialog priv-dialog"
                isCloseButtonShown
                canEscapeKeyClose
            >
                <div className="wc-dialog-body">
                    {psHistoryPath && (
                        <div className="mb-2 p-2 bg-blue-500/10 text-blue-400 text-xs rounded border border-blue-500/20 font-mono break-all">
                            {psHistoryPath}
                        </div>
                    )}
                    {localLoadingMap['psHistView'] && (
                        <div className="flex items-center justify-center py-6 text-[var(--color-text-muted)] text-sm gap-2">
                            <Icon icon="time" /> Loading history…
                        </div>
                    )}
                    {!localLoadingMap['psHistView'] && psHistory.length > 0 && (
                        <div className="mb-2 text-xs text-[var(--color-text-muted)]">
                            Showing <span className="text-[var(--color-accent)] font-mono font-bold">{psHistory.length}</span> of <span className="text-[var(--color-text-primary)] font-mono font-bold">{psHistoryFileTotal.toLocaleString()}</span> total commands
                            {psHistoryFileTotal > psHistory.length && <span className="ml-1 text-[var(--color-warning)]">(last 300 shown)</span>}
                        </div>
                    )}
                    <div style={{ maxHeight: 420, overflowY: "auto", paddingRight: "8px" }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th style={{ width: 40 }}>#</th>
                                    <th>COMMAND</th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {[...psHistory].reverse().map((entry, idx) => (
                                    <motion.tr
                                        key={entry.id}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td className="font-mono text-xs text-[var(--color-text-muted)]">{entry.id + 1}</td>
                                        <td className="font-mono text-[11px]" style={{ wordBreak: "break-all", color: "var(--color-text-primary)" }}>{entry.command}</td>
                                    </motion.tr>
                                ))}
                                {!localLoadingMap['psHistView'] && psHistory.length === 0 && (
                                    <tr><td colSpan={2} className="text-center opacity-50 py-4">No history found</td></tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setPsHistoryDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button
                        icon="trash"
                        text="CLEAR HISTORY"
                        onClick={handleClearPSHistoryFromDialog}
                        loading={localLoadingMap['psHistClear']}
                        className="modal-primary-btn danger"
                        disabled={psHistory.length === 0 && !localLoadingMap['psHistView']}
                    />
                </div>
            </Dialog>

            {/* ── Recent Files Dialog ──────────────────────────────── */}
            <Dialog
                isOpen={recentFilesDialogOpen}
                onClose={() => setRecentFilesDialogOpen(false)}
                title="Recent Files"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="Shell Recent folder — shortcut (.lnk) files pointing to recently accessed documents and folders."
                        intent="primary"
                    />
                    <div style={{ maxHeight: 380, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr><th>NAME</th><th>TARGET</th><th>LAST MODIFIED</th></tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {recentFiles.length === 0 && !localLoadingMap['recentFiles'] && (
                                    <tr><td colSpan={3} className="text-center opacity-50 py-4">No recent file entries found</td></tr>
                                )}
                                {recentFiles.map((f, idx) => (
                                    <motion.tr key={`${f.name}:${f.target ?? ''}:${f.lastModified ?? ''}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td className="font-mono text-[11px]">{f.name}{f.extension !== '.lnk' ? f.extension : ''}</td>
                                        <td className="mono-cell font-mono text-[11px]" style={{ maxWidth: 280, wordBreak: "break-all" }}>{f.target || <span className="opacity-40">—</span>}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-secondary)", whiteSpace: "nowrap" }}>{f.lastModified ? new Date(f.lastModified).toLocaleDateString() : "—"}</td>
                                    </motion.tr>
                                ))}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setRecentFilesDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR ALL" onClick={handleClearRecentFiles} loading={localLoadingMap["recentFilesClear"]} className="modal-primary-btn danger" />
                </div>
            </Dialog>

            {/* ── RDP History Dialog ────────────────────────────────── */}
            <Dialog
                isOpen={rdpHistoryDialogOpen}
                onClose={() => setRdpHistoryDialogOpen(false)}
                title="RDP Connection History"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="Terminal Server Client registry entries, saved credentials (cmdkey), and Default.rdp artifacts."
                        intent="primary"
                    />
                    <div style={{ maxHeight: 380, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr><th>TYPE</th><th>HOST / TARGET</th><th>USERNAME</th></tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {rdpHistory.length === 0 && !localLoadingMap['rdpHistory'] && (
                                    <tr><td colSpan={3} className="text-center opacity-50 py-4">No RDP history found</td></tr>
                                )}
                                {rdpHistory.map((r, idx) => (
                                    <motion.tr key={r.key} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td><span className="text-[10px] px-1.5 py-0.5 rounded font-mono" style={{ background: "var(--color-bg-tertiary)", color: "var(--color-text-muted)" }}>{r.type}</span></td>
                                        <td className="mono-cell font-mono text-[11px]" style={{ maxWidth: 260, wordBreak: "break-all" }}>{r.host}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-secondary)" }}>{r.username || <span className="opacity-40">—</span>}</td>
                                    </motion.tr>
                                ))}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setRdpHistoryDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR ALL" onClick={handleClearRDPHistory} loading={localLoadingMap["rdpHistoryClear"]} className="modal-primary-btn danger" />
                </div>
            </Dialog>

            {/* ── Jump Lists Dialog ─────────────────────────────────── */}
            <Dialog
                isOpen={jumpListsDialogOpen}
                onClose={() => setJumpListsDialogOpen(false)}
                title="Jump Lists"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="AutomaticDestinations and CustomDestinations files. Apps use these to show recently/frequently used items in taskbar right-click menus."
                        intent="primary"
                    />
                    <div style={{ maxHeight: 380, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr><th>FILE</th><th>TYPE</th><th>SIZE (KB)</th><th>LAST MODIFIED</th></tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {jumpLists.length === 0 && !localLoadingMap['jumpLists'] && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">No jump list files found</td></tr>
                                )}
                                {jumpLists.map((j, idx) => (
                                    <motion.tr key={`${j.name}:${j.type}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td className="mono-cell font-mono text-[11px]" style={{ maxWidth: 220, wordBreak: "break-all" }}>{j.name}</td>
                                        <td><span className="text-[10px] px-1.5 py-0.5 rounded" style={{ background: "var(--color-bg-tertiary)", color: j.type === 'Automatic' ? "var(--color-accent)" : "var(--color-warning)" }}>{j.type}</span></td>
                                        <td className="font-mono text-[11px] text-right">{j.sizeKB}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-secondary)", whiteSpace: "nowrap" }}>{j.lastModified ? new Date(j.lastModified).toLocaleDateString() : "—"}</td>
                                    </motion.tr>
                                ))}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setJumpListsDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR ALL" onClick={handleClearJumpLists} loading={localLoadingMap["jumpListsClear"]} className="modal-primary-btn danger" />
                </div>
            </Dialog>

            {/* ── Browser Footprints Dialog ─────────────────────────── */}
            <Dialog
                isOpen={browserFootprintsDialogOpen}
                onClose={() => setBrowserFootprintsDialogOpen(false)}
                title="Browser Footprint Audit"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <div style={{
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 16,
                        padding: "12px 14px",
                        borderRadius: 6,
                        border: "1px solid var(--color-border)",
                        background: "var(--color-bg-tertiary)",
                        marginBottom: 14,
                    }}>
                        <div style={{ minWidth: 0 }}>
                            <div className="text-xs font-semibold" style={{ color: "var(--color-text-primary)" }}>
                                Browser artifact inventory
                            </div>
                            <div className="text-[11px] mt-1" style={{ color: "var(--color-text-muted)", lineHeight: 1.45 }}>
                                History, cookies, caches, sessions, local storage, and login databases by detected browser profile.
                            </div>
                        </div>
                        <div className="flex items-center gap-2 flex-shrink-0">
                            <span className="font-mono text-[11px] px-2 py-1 rounded" style={{ background: "var(--color-bg-secondary)", color: "var(--color-text-secondary)", border: "1px solid var(--color-border)" }}>
                                {browserFootprints.length} browsers
                            </span>
                            <span className="font-mono text-[11px] px-2 py-1 rounded" style={{ background: "color-mix(in srgb, var(--color-danger) 12%, transparent)", color: "var(--color-danger)", border: "1px solid rgba(255,90,69,.25)" }}>
                                {formatSizeKB(browserFootprints.reduce((sum, b) => sum + (b.totalSizeKB || 0), 0))}
                            </span>
                        </div>
                    </div>
                    <div style={{ maxHeight: 500, overflowY: "auto", paddingRight: "6px" }}>
                        {browserFootprints.length === 0 && !localLoadingMap['browserFootprints'] && (
                            <p className="text-xs text-center py-4" style={{ color: "var(--color-text-muted)" }}>No browser profiles detected</p>
                        )}
                        <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 12, paddingBottom: 6 }}>
                            {[...browserFootprints].sort((a, b) => b.totalSizeKB - a.totalSizeKB).map((b) => (
                                <div key={`${b.browser}:${b.profilePath}`} style={{ background: "var(--color-bg-secondary)", borderRadius: 8, padding: 14, border: "1px solid var(--color-border)" }}>
                                    <div className="flex items-start justify-between gap-3 mb-2">
                                        <div style={{ minWidth: 0 }}>
                                            <div className="text-sm font-semibold truncate" style={{ color: "var(--color-text-primary)" }}>{b.browser}</div>
                                            <div className="font-mono text-[10px] truncate mt-0.5" title={b.profilePath} style={{ color: "var(--color-text-muted)" }}>
                                                {b.profilePath}
                                            </div>
                                        </div>
                                        <span className="text-[11px] font-mono px-2 py-1 rounded flex-shrink-0" style={{ background: "color-mix(in srgb, var(--color-danger) 12%, transparent)", color: "var(--color-danger)", border: "1px solid rgba(255,90,69,.25)" }}>
                                            {formatSizeKB(b.totalSizeKB)}
                                        </span>
                                    </div>
                                    <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", columnGap: 12, rowGap: 6, marginTop: 10 }}>
                                        {b.artifacts.filter(a => a.sizeKB > 0).map((a) => (
                                            <div key={`${b.browser}:${b.profilePath}:${a.name}`} style={{ minWidth: 0, padding: "4px 6px", borderRadius: 4, background: "var(--color-bg-tertiary)", border: "1px solid color-mix(in srgb, var(--color-border) 70%, transparent)" }}>
                                                <div className="text-[10px] truncate" title={a.name} style={{ color: "var(--color-text-secondary)" }}>{a.name}</div>
                                                <div className="text-[11px] font-mono mt-0.5" style={{ color: "var(--color-text-muted)", whiteSpace: "nowrap" }}>
                                                    {formatSizeKB(a.sizeKB)}
                                                </div>
                                            </div>
                                        ))}
                                        {b.artifacts.every(a => a.sizeKB === 0) && (
                                            <span className="text-[10px] opacity-40">No data found</span>
                                        )}
                                    </div>
                                    <div className="flex justify-end" style={{ marginTop: 8 }}>
                                        <Button
                                            icon="trash"
                                            text="CLEAR THIS BROWSER"
                                            onClick={() => handleClearBrowserFootprint(b)}
                                            loading={localLoadingMap[browserFootprintCacheKey(b)]}
                                            className="modal-primary-btn danger"
                                        />
                                    </div>
                                </div>
                            ))}
                        </div>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setBrowserFootprintsDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR ALL BROWSERS" onClick={handleClearBrowserFootprints} loading={localLoadingMap["browserFootprintsClear"]} className="modal-primary-btn danger" />
                </div>
            </Dialog>

            {/* ── Prefetch Files Dialog ─────────────────────────────── */}
            <Dialog
                isOpen={prefetchDialogOpen}
                onClose={() => setPrefetchDialogOpen(false)}
                title="Prefetch Files"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="Windows Prefetch files record application execution history. Each .pf file proves an executable was launched on this device."
                        intent="primary"
                    />
                    <div style={{ maxHeight: 400, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr><th>APP</th><th>LAST RUN</th><th>SIZE (KB)</th></tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {prefetchFiles.length === 0 && !localLoadingMap['prefetch'] && (
                                    <tr><td colSpan={3} className="text-center opacity-50 py-4">
                                        {prefetchAccessDenied
                                            ? "Access denied — launch WinCommander as Administrator"
                                            : prefetcherEnabled === 0
                                                ? "Prefetch is fully disabled (EnablePrefetcher=0)"
                                                : prefetcherEnabled === 1
                                                    ? "Boot prefetch only — application prefetch is disabled (EnablePrefetcher=1)"
                                                    : prefetcherEnabled === 2
                                                        ? "No prefetch files recorded yet"
                                                        : "No prefetch files found"}
                                    </td></tr>
                                )}
                                {prefetchFiles.map((p, idx) => (
                                    <motion.tr key={p.fileName} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td className="font-mono text-[11px]">{p.name}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-secondary)", whiteSpace: "nowrap" }}>{p.lastRun ? new Date(p.lastRun).toLocaleString() : "—"}</td>
                                        <td className="font-mono text-[11px] text-right">{p.sizeKB}</td>
                                    </motion.tr>
                                ))}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                    {prefetchFiles.length > 0 && (
                        <p className="text-[10px] mt-2" style={{ color: "var(--color-text-muted)" }}>{prefetchFiles.length} prefetch files found</p>
                    )}
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setPrefetchDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR PREFETCH" onClick={handleClearPrefetch} loading={localLoadingMap["prefetchClear"]} className="modal-primary-btn danger" />
                </div>
            </Dialog>

            {/* ── Shadow Copies Dialog ──────────────────────────────── */}
            <Dialog
                isOpen={shadowCopiesDialogOpen}
                onClose={() => setShadowCopiesDialogOpen(false)}
                title="Volume Shadow Copies"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="VSS snapshots. Cleanup investigators use these to recover deleted files or view past states of the filesystem."
                        intent="warning"
                    />
                    <div style={{ maxHeight: 380, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr><th>DRIVE</th><th>CREATED</th><th>STATE</th><th>PERSISTENT</th></tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {shadowCopies.length === 0 && !localLoadingMap['shadowCopies'] && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">
                                        {vssRunning
                                            ? "VSS is running but no snapshots exist — no restore points have been created"
                                            : "VSS service is not running — shadow copies unavailable"}
                                    </td></tr>
                                )}
                                {shadowCopies.map((s, idx) => (
                                    <motion.tr key={s.id} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td className="font-mono text-[11px]">{s.drive}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-secondary)", whiteSpace: "nowrap" }}>{s.created || "—"}</td>
                                        <td><span className="text-[10px] px-1.5 py-0.5 rounded font-mono" style={{ background: "var(--color-bg-tertiary)", color: "var(--color-text-muted)" }}>{s.stateStr}</span></td>
                                        <td className="text-xs text-center">{s.persistent ? "Yes" : "No"}</td>
                                    </motion.tr>
                                ))}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setShadowCopiesDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="DELETE ALL COPIES" onClick={handleClearShadowCopies} loading={localLoadingMap["shadowCopiesClear"]} className="modal-primary-btn danger" />
                </div>
            </Dialog>

            {/* ── NTFS Journals Dialog ──────────────────────────────── */}
            <Dialog
                isOpen={ntfsJournalsDialogOpen}
                onClose={() => setNtfsJournalsDialogOpen(false)}
                title="NTFS USN Journals"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message="USN Change Journals record every file create/modify/delete operation on NTFS volumes. Cleanup tools can use them to reconstruct file activity."
                        intent="primary"
                    />
                    <div style={{ maxHeight: 300, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr><th>DRIVE</th><th>STATUS</th><th>JOURNAL ID</th><th>MAX SIZE</th></tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {ntfsJournals.length === 0 && !localLoadingMap['ntfsJournals'] && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">No drives found (requires admin)</td></tr>
                                )}
                                {ntfsJournals.map((j, idx) => (
                                    <motion.tr key={j.drive} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td className="font-mono text-[12px] font-semibold">{j.drive}</td>
                                        <td>
                                            <span className="text-[10px] px-1.5 py-0.5 rounded font-mono" style={{
                                                background: j.present ? "color-mix(in srgb, var(--color-danger) 15%, transparent)" : "var(--color-bg-tertiary)",
                                                color: j.present ? "var(--color-danger)" : "var(--color-text-muted)"
                                            }}>
                                                {j.present ? "ACTIVE" : "NOT FOUND"}
                                            </span>
                                        </td>
                                        <td className="font-mono text-[11px]">{j.journalId || "—"}</td>
                                        <td className="font-mono text-[11px]">{j.maxSize || "—"}</td>
                                    </motion.tr>
                                ))}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setNtfsJournalsDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button
                        icon="trash"
                        text="DELETE ALL JOURNALS"
                        onClick={handleClearNTFSJournals}
                        loading={localLoadingMap["ntfsJournalsClear"]}
                        className="modal-primary-btn danger"
                        disabled={!ntfsJournals.some(j => j.present)}
                    />
                </div>
            </Dialog>
            {/* ── Amcache Viewer Dialog ───────────────────────────── */}
            <Dialog
                isOpen={amcacheDialogOpen}
                onClose={() => setAmcacheDialogOpen(false)}
                title="Amcache — Execution Traces"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message={amcacheData ? `Amcache.hve: ${amcacheData.hveFileSizeMb} MB — ${amcacheData.total} live registry categories. File timestamps, hashes and execution paths persist across reboots.` : "Loading Amcache data..."}
                        intent="warning"
                    />
                    <div style={{ maxHeight: 340, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead><tr><th>CATEGORY</th><th>LIVE KEYS</th><th>SAMPLE NAME</th></tr></thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {!amcacheData && localLoadingMap['amcacheView'] && (
                                    <tr><td colSpan={3} className="text-center opacity-50 py-4">Loading...</td></tr>
                                )}
                                {amcacheData && amcacheData.hveFileExists && (
                                    <tr>
                                        <td className="font-mono text-[11px] font-semibold" style={{ color: "var(--color-danger)" }}>Amcache.hve FILE</td>
                                        <td className="font-mono text-[11px]">{amcacheData.hveFileSizeMb} MB</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-muted)" }}>AppCompatCache hive on disk</td>
                                    </tr>
                                )}
                                {amcacheData?.entries.map((e, idx) => (
                                    <motion.tr key={e.category} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td className="font-mono text-[11px]">{e.category}</td>
                                        <td className="font-mono text-[11px]">{e.count}</td>
                                        <td className="text-xs truncate max-w-[200px]" style={{ color: "var(--color-text-muted)" }}>{e.sample?.[0]?.name || "—"}</td>
                                    </motion.tr>
                                ))}
                                {amcacheData && amcacheData.entries.length === 0 && !amcacheData.hveFileExists && (
                                    <tr><td colSpan={3} className="text-center opacity-50 py-4">No Amcache data found</td></tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setAmcacheDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="PURGE AMCACHE" onClick={() => { setAmcacheDialogOpen(false); handleClearAmcache(); }} className="modal-primary-btn danger" />
                </div>
            </Dialog>

            {/* ── NTUSER Traces Viewer Dialog ──────────────────────── */}
            <Dialog
                isOpen={ntUserTracesDialogOpen}
                onClose={() => setNtUserTracesDialogOpen(false)}
                title="NTUSER Traces — MRU & Typed History"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message={ntUserTracesData ? `${ntUserTracesData.total} total entries across RunMRU, TypedPaths, TypedURLs, and WordWheelQuery. All reveal user activity patterns.` : "Loading NTUSER traces..."}
                        intent="warning"
                    />
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        {ntUserTracesData?.sections.map((section) => (
                            <div key={section.name} style={{ marginBottom: 16 }}>
                                <div className="text-[10px] font-semibold uppercase tracking-wider mb-1" style={{ color: "var(--color-text-muted)" }}>{section.name} ({section.count})</div>
                                {section.entries.length === 0 ? (
                                    <div className="text-xs opacity-40 pl-2">Empty</div>
                                ) : (
                                    <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                                        <thead><tr><th>KEY</th><th>VALUE</th></tr></thead>
                                        <AnimatePresence initial={false}>
                                        <tbody>
                                            {section.entries.map((entry, ei) => (
                                                <motion.tr key={`${section.name}:${entry.key}:${entry.value}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(ei), duration: DURATION_S.fast }}>
                                                    <td className="font-mono text-[11px]">{entry.key}</td>
                                                    <td className="text-xs truncate max-w-[240px]" style={{ color: "var(--color-text-secondary)" }}>{entry.value}</td>
                                                </motion.tr>
                                            ))}
                                        </tbody>
                                        </AnimatePresence>
                                    </table>
                                )}
                            </div>
                        ))}
                        {!ntUserTracesData && !localLoadingMap['ntUserTracesView'] && (
                            <div className="text-center opacity-40 py-4">No data</div>
                        )}
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setNtUserTracesDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR ALL TRACES" onClick={() => { setNtUserTracesDialogOpen(false); handleClearNTUserTraces(); }} className="modal-primary-btn danger" />
                </div>
            </Dialog>

            {/* ── Notepad State Viewer Dialog ──────────────────────── */}
            <Dialog
                isOpen={notepadStateDialogOpen}
                onClose={() => setNotepadStateDialogOpen(false)}
                title="Notepad Tab State Files"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message={notepadStateData ? `${notepadStateData.total} tab state files (${notepadStateData.totalSizeKB.toFixed(1)} KB). These .bin files contain content of unsaved Notepad tabs.` : "Loading Notepad state files..."}
                        intent={notepadStateData && notepadStateData.total > 0 ? "warning" : "primary"}
                    />
                    <div style={{ maxHeight: 320, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead><tr><th>FILE</th><th>SIZE (KB)</th><th>MODIFIED</th></tr></thead>
                            <tbody>
                                {!notepadStateData && localLoadingMap['notepadStateView'] && (
                                    <tr><td colSpan={3} className="text-center opacity-50 py-4">Loading...</td></tr>
                                )}
                                {notepadStateData?.files.map((f, idx) => (
                                    <motion.tr key={f.name} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td className="font-mono text-[10px] truncate max-w-[200px]">{f.name}</td>
                                        <td className="font-mono text-[11px]">{f.sizeKB}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-muted)" }}>{f.modified}</td>
                                    </motion.tr>
                                ))}
                                {notepadStateData && notepadStateData.total === 0 && (
                                    <tr><td colSpan={3} className="text-center opacity-50 py-4">No Notepad tab state files found</td></tr>
                                )}
                            </tbody>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setNotepadStateDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="PURGE STATE" onClick={() => { setNotepadStateDialogOpen(false); handleClearNotepadState(); }} loading={localLoadingMap["notepadState"]} className="modal-primary-btn danger" disabled={!notepadStateData?.total} />
                </div>
            </Dialog>

            {/* ── PCA Info Viewer Dialog ───────────────────────────── */}
            <Dialog
                isOpen={pcaInfoDialogOpen}
                onClose={() => setPcaInfoDialogOpen(false)}
                title="PCA — Program Compatibility Assistant"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message={pcaInfoData ? `${pcaInfoData.total} file(s), ${pcaInfoData.totalSizeMB} MB — PcaSvc: ${pcaInfoData.pcaSvcState}. PCA logs every program launch and compatibility fix.` : "Loading PCA data..."}
                        intent="warning"
                    />
                    <div style={{ maxHeight: 320, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead><tr><th>FILE</th><th>TYPE</th><th>SIZE (KB)</th><th>MODIFIED</th></tr></thead>
                            <tbody>
                                {!pcaInfoData && localLoadingMap['pcaInfoView'] && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">Loading...</td></tr>
                                )}
                                {pcaInfoData?.files.map((f, idx) => (
                                    <motion.tr key={`${f.name}:${f.type}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td className="font-mono text-[11px]">{f.name}</td>
                                        <td className="font-mono text-[10px]">{f.type}</td>
                                        <td className="font-mono text-[11px]">{f.sizeKB}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-muted)" }}>{f.modified}</td>
                                    </motion.tr>
                                ))}
                                {pcaInfoData && pcaInfoData.total === 0 && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">No PCA database files found</td></tr>
                                )}
                            </tbody>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setPcaInfoDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR PCA DB" onClick={() => { setPcaInfoDialogOpen(false); handleClearPCADatabase(); }} loading={localLoadingMap["pcaDatabase"]} className="modal-primary-btn danger" disabled={!pcaInfoData?.total} />
                </div>
            </Dialog>

            {/* ── Crash Dumps Viewer Dialog ────────────────────────── */}
            <Dialog
                isOpen={crashDumpsDialogOpen}
                onClose={() => setCrashDumpsDialogOpen(false)}
                title="Crash Dumps — WER Reports & Minidumps"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message={crashDumpsData ? `${crashDumpsData.total} dump file(s), ${crashDumpsData.totalSizeMB.toFixed(2)} MB. Crash dumps may contain process memory snapshots with sensitive data.` : "Loading crash dumps..."}
                        intent={crashDumpsData && crashDumpsData.total > 0 ? "warning" : "primary"}
                    />
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead><tr><th>SOURCE</th><th>FILE</th><th>SIZE (KB)</th><th>MODIFIED</th></tr></thead>
                            <tbody>
                                {!crashDumpsData && localLoadingMap['crashDumpsView'] && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">Loading...</td></tr>
                                )}
                                {crashDumpsData?.dumps.map((d, idx) => (
                                    <motion.tr key={`${d.source}:${d.name}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td><span className="text-[10px] px-1.5 py-0.5 rounded font-mono" style={{ background: "var(--color-bg-tertiary)", color: "var(--color-text-muted)" }}>{d.source}</span></td>
                                        <td className="text-xs truncate max-w-[180px]">{d.name}</td>
                                        <td className="font-mono text-[11px]">{d.sizeKB}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-muted)" }}>{d.modified}</td>
                                    </motion.tr>
                                ))}
                                {crashDumpsData && crashDumpsData.total === 0 && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">No crash dumps found</td></tr>
                                )}
                            </tbody>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setCrashDumpsDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR ALL DUMPS" onClick={() => { setCrashDumpsDialogOpen(false); handleCrashDumpErase(); }} loading={localLoadingMap["crashDumps"]} className="modal-primary-btn danger" disabled={!crashDumpsData?.total} />
                </div>
            </Dialog>

            {/* ── Recall / Timeline DB Viewer Dialog ──────────────── */}
            <Dialog
                isOpen={recallDbDialogOpen}
                onClose={() => setRecallDbDialogOpen(false)}
                title="Recall + Timeline Databases"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message={recallDbData ? `${recallDbData.total} database file(s), ${recallDbData.totalSizeMB.toFixed(2)} MB. Includes Recall snapshots, activity history, and connected device logs.` : "Loading Recall/Timeline databases..."}
                        intent={recallDbData && recallDbData.total > 0 ? "warning" : "primary"}
                    />
                    <div style={{ maxHeight: 340, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead><tr><th>SOURCE</th><th>FILE</th><th>SIZE (KB)</th><th>MODIFIED</th></tr></thead>
                            <tbody>
                                {!recallDbData && localLoadingMap['recallDbView'] && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">Loading...</td></tr>
                                )}
                                {recallDbData?.databases.map((db, idx) => (
                                    <motion.tr key={`${db.source}:${db.name}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td><span className="text-[10px] px-1.5 py-0.5 rounded font-mono" style={{ background: "var(--color-bg-tertiary)", color: "var(--color-text-muted)" }}>{db.source}</span></td>
                                        <td className="text-xs truncate max-w-[200px]">{db.name}</td>
                                        <td className="font-mono text-[11px]">{db.sizeKB}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-muted)" }}>{db.modified}</td>
                                    </motion.tr>
                                ))}
                                {recallDbData && recallDbData.total === 0 && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">No Recall/Timeline databases found</td></tr>
                                )}
                            </tbody>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setRecallDbDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="PURGE DATABASES" onClick={() => { setRecallDbDialogOpen(false); handleClearRecallDatabase(); }} loading={localLoadingMap["recallDatabase"]} className="modal-primary-btn danger" disabled={!recallDbData?.total} />
                </div>
            </Dialog>
            {/* ── Search Index Viewer Dialog ───────────────────────── */}
            <Dialog
                isOpen={searchIndexDialogOpen}
                onClose={() => setSearchIndexDialogOpen(false)}
                title="Windows Search Index"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message={searchIndexData
                            ? `${searchIndexData.total} file(s), ${searchIndexData.totalSizeMB} MB — WSearch: ${searchIndexData.wsearchState}. Windows.edb contains indexed content from every searched file.`
                            : "Loading search index info..."}
                        intent={searchIndexData && searchIndexData.total > 0 ? "warning" : "primary"}
                    />
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead><tr><th>LOCATION</th><th>FILE</th><th>SIZE (KB)</th><th>MODIFIED</th></tr></thead>
                            <tbody>
                                {!searchIndexData && localLoadingMap['searchIndexView'] && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">Loading...</td></tr>
                                )}
                                {searchIndexData?.files.map((f, idx) => (
                                    <motion.tr key={`${f.label}:${f.name}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td><span className="text-[10px] px-1.5 py-0.5 rounded font-mono" style={{ background: "var(--color-bg-tertiary)", color: "var(--color-text-muted)" }}>{f.label}</span></td>
                                        <td className="text-xs truncate max-w-[180px]">{f.name}</td>
                                        <td className="font-mono text-[11px]">{f.sizeKB}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-muted)" }}>{f.modified}</td>
                                    </motion.tr>
                                ))}
                                {searchIndexData && searchIndexData.total === 0 && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">No search index files found</td></tr>
                                )}
                            </tbody>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setSearchIndexDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR INDEX" onClick={() => { setSearchIndexDialogOpen(false); handleClearSearchIndex(); }} loading={localLoadingMap["searchIndex"]} className="modal-primary-btn danger" disabled={!searchIndexData?.total} />
                </div>
            </Dialog>

            {/* ── Print Spooler Viewer Dialog ──────────────────────── */}
            <Dialog
                isOpen={printSpoolerDialogOpen}
                onClose={() => setPrintSpoolerDialogOpen(false)}
                title="Print Spooler — Queued Documents"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message={printSpoolerData
                            ? `${printSpoolerData.total} file(s), ${printSpoolerData.totalSizeMB} MB — Spooler: ${printSpoolerData.spoolerState}. Spool files contain verbatim document images of queued print jobs.`
                            : "Loading print spooler info..."}
                        intent={printSpoolerData && printSpoolerData.total > 0 ? "warning" : "primary"}
                    />
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead><tr><th>SOURCE</th><th>FILE</th><th>SIZE (KB)</th><th>MODIFIED</th></tr></thead>
                            <tbody>
                                {!printSpoolerData && localLoadingMap['printSpoolerView'] && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">Loading...</td></tr>
                                )}
                                {printSpoolerData?.files.map((f, idx) => (
                                    <motion.tr key={`${f.source}:${f.name}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td><span className="text-[10px] px-1.5 py-0.5 rounded font-mono" style={{ background: "var(--color-bg-tertiary)", color: "var(--color-text-muted)" }}>{f.source}</span></td>
                                        <td className="text-xs truncate max-w-[200px]">{f.name}</td>
                                        <td className="font-mono text-[11px]">{f.sizeKB}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-muted)" }}>{f.modified}</td>
                                    </motion.tr>
                                ))}
                                {printSpoolerData && printSpoolerData.total === 0 && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">Print spool queue is empty</td></tr>
                                )}
                            </tbody>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setPrintSpoolerDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR SPOOLER" onClick={() => { setPrintSpoolerDialogOpen(false); handleClearPrintSpooler(); }} loading={localLoadingMap["printSpooler"]} className="modal-primary-btn danger" disabled={!printSpoolerData?.total} />
                </div>
            </Dialog>

            {/* ── SQLite WAL Files Viewer Dialog ───────────────────── */}
            <Dialog
                isOpen={walFilesDialogOpen}
                onClose={() => setWalFilesDialogOpen(false)}
                title="Temp Database Files"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <UniversalCallout
                        message={walFilesData ? `${walFilesData.total} .wal/.shm file(s), ${walFilesData.totalSizeMB.toFixed(2)} MB. WAL files contain recent uncommitted database transactions — often readable after an app closes.` : "Scanning for WAL files..."}
                        intent={walFilesData && walFilesData.total > 0 ? "warning" : "primary"}
                    />
                    <div style={{ maxHeight: 360, overflowY: "auto", paddingRight: "8px", marginTop: 12 }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead><tr><th>FILE</th><th>SIZE (KB)</th><th>DIR</th><th>MODIFIED</th></tr></thead>
                            <tbody>
                                {!walFilesData && localLoadingMap['walFilesView'] && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">Scanning...</td></tr>
                                )}
                                {walFilesData?.files.map((f, idx) => (
                                    <motion.tr key={`${f.dir}:${f.name}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}>
                                        <td className="font-mono text-[11px]">{f.name}</td>
                                        <td className="font-mono text-[11px]">{f.sizeKB}</td>
                                        <td className="text-xs truncate max-w-[180px]" style={{ color: "var(--color-text-muted)" }}>{f.dir}</td>
                                        <td className="text-xs" style={{ color: "var(--color-text-muted)" }}>{f.modified}</td>
                                    </motion.tr>
                                ))}
                                {walFilesData && walFilesData.total === 0 && (
                                    <tr><td colSpan={4} className="text-center opacity-50 py-4">No .wal or .shm files found — system is clean</td></tr>
                                )}
                            </tbody>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setWalFilesDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="KILL ALL WAL" onClick={() => { setWalFilesDialogOpen(false); handleSQLiteWALKiller(); }} loading={localLoadingMap["walKiller"]} className="modal-primary-btn danger" disabled={!walFilesData?.total} />
                </div>
            </Dialog>
        </>
    ) };
}
