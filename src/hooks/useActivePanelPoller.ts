// src/hooks/useActivePanelPoller.ts
//
// Active-Panel Polling — only polls data for the panel the user is currently viewing.
// Replaces the old "poll everything always" approach that pinned CPU at 80-90%.
//
// Behavior:
//   1. On panel ENTER: immediate one-shot refresh (fresh data on arrival)
//   2. While on panel: silent refresh every 10s
//   3. On panel LEAVE: interval is cleared (zero CPU for hidden panels)
//   4. Dashboard special case: refreshLiveMetrics runs every 2s (Rust-native,
//      zero PS spawns) so CPU/RAM gauges stay live.
//
// Panels without auto-poll (apps, cleanup, vault, server-apps, search-files, system-identity):
//   → No interval created. User uses manual refresh button.

import { useEffect, useRef, useCallback } from 'react';
import type { PanelId } from '../types/panels';
import { PANEL_MANIFESTS } from '../types/panels';
import { useAppState } from '../context/AppContext';
import { useLiveMetrics } from '../context/LiveMetricsContext';
import { getModuleForPanel, isModuleEnabled } from '../types/modules';

const LIVE_METRICS_INTERVAL = 2_000;  // 2s — Rust sysinfo, <1ms cost
// Drive health is fetched once per app lifetime — smartctl+PS spin up ~1s
// and SMART data doesn't change in seconds. Module-level flag survives
// dashboard remounts (panel switch back & forth) so we don't re-run.
let driveHealthChecked = false;
const PANEL_POLL_INTERVAL = 10_000; // 10s — PS-backed panel data
const MANUAL_REFRESH_PANELS = new Set<PanelId>([
    'apps',
    'cleanup',
    'vault',
    'server-apps',
    'search-files',
    'system-identity',
    'advisor',
    'flows',
]);

type RefreshInFlightRef = { current: boolean };
type RefreshResult = 'ran' | 'skipped';

export function shouldAutoPollPanel(panel: PanelId, refreshKey?: string): boolean {
    return Boolean(refreshKey) && !MANUAL_REFRESH_PANELS.has(panel);
}

export function getAutoPollRefreshKey(panel: PanelId): string | null {
    const refreshKey = PANEL_MANIFESTS.find((m) => m.id === panel)?.refreshKey;
    return shouldAutoPollPanel(panel, refreshKey) ? refreshKey! : null;
}

export function getDashboardPanelRefreshKeys(): string[] {
    const refreshKey = getAutoPollRefreshKey('dashboard');
    return refreshKey ? [refreshKey] : [];
}

export async function runRefreshIfIdle(
    inFlight: RefreshInFlightRef,
    refresh: () => Promise<void> | void,
): Promise<RefreshResult> {
    if (inFlight.current) return 'skipped';
    inFlight.current = true;
    try {
        await refresh();
        return 'ran';
    } finally {
        inFlight.current = false;
    }
}

export function useActivePanelPoller({ activePanel, paused = false }: { activePanel: PanelId; paused?: boolean }) {
    const appState = useAppState();
    const {
        appSettings,
        refreshSystem,
        refreshDriveHealth,
        refreshPrivacy,
        refreshNetwork,
        refreshDashboard,
        refreshHardening,
        refreshMesh,
        refreshProductivity,
    } = appState;
    const { refreshLiveMetrics } = useLiveMetrics();

    // Track previous panel to fire immediate one-shot on panel change
    const prevPanelRef = useRef<PanelId | null>(null);
    const panelRefreshInFlightRef = useRef(false);

    // ── Manifest-driven refresh lookup ───────────────────────────────────
    // Instead of a switch-case mapping panel → refresh function, we look up
    // the manifest's refreshKey and grab the function from appState dynamically.
    // Adding a new panel with auto-poll = just set refreshKey in its manifest.
    const getPanelRefresh = useCallback((): (() => Promise<void>) | null => {
        const refreshKey = getAutoPollRefreshKey(activePanel);
        if (!refreshKey) return null;
        // Module gate — if the panel's module is disabled, skip all backend work
        const mod = getModuleForPanel(activePanel);
        if (mod && !isModuleEnabled(appSettings?.app?.modules, mod)) return null;
        const refreshers: Record<string, ((silent?: boolean) => Promise<void>) | undefined> = {
            refreshDashboard,
            refreshPrivacy,
            refreshNetwork,
            refreshHardening,
            refreshMesh,
            refreshProductivity,
        };
        const fn = refreshers[refreshKey];
        if (typeof fn !== 'function') return null;
        // Most refresh functions accept a `silent` boolean param
        return () => fn(true);
    }, [
        activePanel,
        appSettings?.app?.modules,
        refreshDashboard,
        refreshHardening,
        refreshMesh,
        refreshNetwork,
        refreshPrivacy,
        refreshProductivity,
    ]);

    // KT: Entering idle-pause should force a fresh one-shot refresh when resuming.
    // Clearing prevPanelRef guarantees panel-entry logic runs again after pause lifts.
    useEffect(() => {
        if (paused) {
            prevPanelRef.current = null;
        }
    }, [paused]);

    // ── Dashboard: Rust-native live metrics at 2s (CPU / RAM / Disk) ─────
    useEffect(() => {
        if (paused) return;
        if (activePanel !== 'dashboard') return;

        // Fire immediately so gauges populate at once without waiting 2s
        refreshLiveMetrics();

        const id = setInterval(() => refreshLiveMetrics(), LIVE_METRICS_INTERVAL);
        return () => clearInterval(id);
    }, [activePanel, refreshLiveMetrics, paused]);

    // ── Dashboard: SMART disk health — once per app lifetime ─────────────
    // Per user feedback: SMART values don't change second-to-second, and the
    // smartctl probe is heavy enough that re-running it on every dashboard
    // remount visibly slowed the panel. Fire once total; re-launch the app
    // to refresh.
    useEffect(() => {
        if (paused) return;
        if (activePanel !== 'dashboard') return;
        if (driveHealthChecked) return;
        driveHealthChecked = true;
        refreshDriveHealth();
    }, [activePanel, refreshDriveHealth, paused]);

    // ── All panels: PS-backed data at 10s (only the active panel) ──────────
    useEffect(() => {
        if (paused) return;
        const refreshFn = getPanelRefresh();

        // ── One-shot on panel entry ──
        if (prevPanelRef.current !== activePanel) {
            prevPanelRef.current = activePanel;
            if (activePanel === 'dashboard') {
                // Populate static hardware info (cpu model, hostname, RAM string, etc.)
                refreshSystem(true);
            }
            if (refreshFn) void runRefreshIfIdle(panelRefreshInFlightRef, refreshFn);
        }

        // ── Periodic refresh while panel is active ──
        if (!refreshFn) return;

        const id = setInterval(() => {
            void runRefreshIfIdle(panelRefreshInFlightRef, refreshFn);
        }, PANEL_POLL_INTERVAL);
        return () => clearInterval(id);

    }, [
        activePanel,
        getPanelRefresh,
        paused,
        refreshSystem,
    ]);
}
