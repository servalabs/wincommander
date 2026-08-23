import { lazy, Suspense, useCallback, useEffect, useMemo, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppState } from "../../context/AppContext";
import { useLiveMetrics } from "../../context/LiveMetricsContext";
import useVisibility from "../../hooks/useVisibility";
import { executeBackendCommand, useBackend } from "../../hooks/useBackend";
import { runOperation } from "../../context/OperationContext";
import SovereigntyRadar from "../../components/dashboard/SovereigntyRadar";
import ViewToggle from "../../components/dashboard/ViewToggle";
import NeedsAttention from "../../components/dashboard/NeedsAttention";
import type { ScanFinding } from "../../components/startup/WizardAnimations";
import { useDashboardRadar } from "../../hooks/useDashboardRadar";
import { AnimatePresence, motion } from "framer-motion";
import PrivacyTogglesCard from "../../components/dashboard/PrivacyTogglesCard";
import HardwareSpecsCard from "../../components/dashboard/HardwareSpecsCard";
import StorageOverviewCard from "../../components/dashboard/StorageOverviewCard";
import RecentDownloadsCard from "../../components/dashboard/RecentDownloadsCard";
import NetworkTrafficCard from "../../components/dashboard/NetworkTrafficCard";
import TierGate from "../../components/shared/TierGate";
import UpdaterStatus from "../../components/UpdaterStatus";
import UpdateFlowDialog from "../../components/UpdateFlowDialog";
import { useUpdater } from "../../hooks/updaterStore";
import AppsUpdateButton from "../../components/AppsUpdateButton";
import { useSovereigntyScore } from "../../hooks/useSovereigntyScore";
import useEntitlements from "../../hooks/useEntitlements";
import useProInstall from "../../hooks/useProInstall";
import useBorrowedActive from "../../hooks/useBorrowedActive";
import { useQueuedAppUpdateIds } from "../../hooks/useQueuedAppUpdateIds";
import { markAppUpdatesQueued, clearAppUpdatesQueued } from "../../lib/appUpdateQueue";
import { releasePackageOperation, tryAcquirePackageOperation } from "../../lib/packageOperationLock";
import { getRadarDriftToggles, getToggleById } from "../../registry";
import { getByPath, getToggleVisibility, resolveToggleText } from "../../types/toggles";
import { getToggleDrift } from "../../lib/toggleDrift";
import { getDisplayBranding } from "../../lib/branding";
import { useTaskStatus } from "../../context/TaskStatusContext";
import { Icon } from "../../components/ui/icon";
import { showWarning } from "../../utils/toast";
// Motion SSOT — never hardcode durations or curves directly in JSX.
import { DURATION_S, EASE } from "../../components/shared/motion";
import './index.css';

const SovereigntyRiskMatrix = lazy(() => import("../../components/SovereigntyRiskMatrix"));
const MoreProductsView = lazy(() => import("../../components/dashboard/MoreProductsView"));

// Right-column collapsible card group: at most two expanded at once.
const MAX_OPEN_CARDS = 2;
// All right-column cards start collapsed (owner request): System Info shows only
// the CPU/RAM bars, Storage only its bar, Network Traffic only the speed.
const DEFAULT_OPEN_CARDS: string[] = [];
// Only OPTIONAL / panel-gated engines belong here — they are surfaced within
// their own panel, not in the dashboard "Needs Attention" list. Never add a
// CRITICAL engine (see CRITICAL_ENGINES in apps/components/EnginesSection.tsx:
// powershell7, ramDiskEngine, …) or a genuinely-missing
// critical dependency will be silently hidden from the dashboard radar/score.
const DASHBOARD_HIDDEN_DEPS = new Set(['productivityEngine', 'meshVpn', 'localLlm', 'chocolatey', 'scoop']);

function compareVersions(a: string | null | undefined, b: string | null | undefined): number | null {
  const parse = (value: string | null | undefined): number[] | null => {
    const match = (value ?? "").trim().replace(/^v/i, "").match(/\d+(?:\.\d+){0,3}/);
    if (!match) return null;
    return match[0].split(".").map((part) => Number.parseInt(part, 10));
  };
  const left = parse(a);
  const right = parse(b);
  if (!left || !right) return null;
  const len = Math.max(left.length, right.length);
  for (let i = 0; i < len; i++) {
    const diff = (left[i] ?? 0) - (right[i] ?? 0);
    if (diff !== 0) return diff > 0 ? 1 : -1;
  }
  return 0;
}

export default function DashboardPanel() {
  const containerRef = useRef<HTMLDivElement>(null);
  const windowScale = 1; // Handled globally by AppShell
  const {
    systemInfo,
    loading,
    appSettings,
    appInventory,
    refreshNetwork,
    refreshSettings,
    refreshDependencies,
    patchAppSettings,
    dependencyStatus,
  } = useAppState();
  const { metrics: liveMetrics, status: liveMetricsStatus } = useLiveMetrics();
  const dashboardSystemInfo = useMemo(() => {
    if (!liveMetrics) return systemInfo;
    const staticInfo = systemInfo ?? {
      osName: '', osVersion: '', buildNumber: '', hostname: '',
      isAdmin: false, cpu: '', cpuUsage: 0, cpuTemp: 0, ram: `${liveMetrics.ramTotalGb} GB`,
      ramUsage: 0, gpu: '', disks: [], uptime: { days: 0, hours: 0, minutes: 0 },
    };
    const healthByDisk = new Map(staticInfo.disks.map((disk) => [disk.id, disk.healthPercent]));
    return {
      ...staticInfo,
      ...liveMetrics,
      cpuTemp: liveMetrics.cpuTemp ?? staticInfo.cpuTemp,
      disks: liveMetrics.disks.map((disk) => ({
        ...disk,
        healthPercent: healthByDisk.get(disk.id) ?? null,
      })),
    };
  }, [systemInfo, liveMetrics]);

  const {
    toggleContextMenu,
    getContextMenuStatus,
    toggleScrubContextMenu,
    getScrubContextMenuStatus,
    testWingetInstalled,
    installWinget,
    getAppInventory,
    upgradeApp,
    invokeDiskCleanup,
  } = useBackend();


  const visibility = useVisibility();
  const { hasPaid, canUse } = useEntitlements();
  const pro = useProInstall({
    status: hasPaid,
    manifest: hasPaid,
    defender: false,
  });
  const updater = useUpdater();
  const [updateFlowOpen, setUpdateFlowOpen] = useState(false);
  const score = useSovereigntyScore();
  const radar = useDashboardRadar();
  const { tasks: activeTasks } = useTaskStatus();
  const isAppUpdateTaskRunning = activeTasks.some(
    (t) => t.status === "running" && /^Update \d+ Apps?$/.test(t.label)
  );
  // Fix Everything is the umbrella — it includes app-update work. While it runs,
  // the standalone "Update All Apps" button must not start a duplicate task.
  const isFixEverythingRunning = activeTasks.some(
    (t) => t.status === "running" && t.label === "Fix Everything"
  );
  // This directly-owned flag covers the interval before TaskStatusContext
  // registers the operation, preventing duplicate Fix All submissions.
  const [isFixAllRunning, setIsFixAllRunning] = useState(false);
  const fixAllInProgress = isFixEverythingRunning || isFixAllRunning;
  // App-update ids claimed by any in-flight upgrade (marked synchronously at
  // every queuing site). Lets us drop app-update findings the instant they're
  // queued — before the update task even registers as "running".
  const queuedAppUpdateIds = useQueuedAppUpdateIds();
  const branding = getDisplayBranding(appSettings);

  const [displayScore, setDisplayScore] = useState(0);
  // Dashboard is the stable default. A legacy saved "map" value resolves to
  // Dashboard because Live Map is no longer a navigation state.
  const DASHBOARD_VIEW_STORAGE_KEY = "wincommander.dashboard-view";
  const persistedViewMode = appSettings?.app?.dashboardViewMode;
  const initialDashboardView = window.sessionStorage.getItem(DASHBOARD_VIEW_STORAGE_KEY);
  const [viewMode, setViewModeState] = useState<"dashboard" | "risk" | "products">(
    initialDashboardView === "risk" || initialDashboardView === "products"
      ? initialDashboardView
      : persistedViewMode === "risk" || persistedViewMode === "products"
        ? persistedViewMode
        : "dashboard"
  );
  const setViewMode = useCallback((mode: "dashboard" | "risk" | "products") => {
    setViewModeState(mode);
    window.sessionStorage.setItem(DASHBOARD_VIEW_STORAGE_KEY, mode);
    patchAppSettings({ app: { dashboardViewMode: mode } }).catch(() => { });
  }, [patchAppSettings]);
  useEffect(() => {
    const handleDashboardView = (event: Event) => {
      const mode = (event as CustomEvent<"dashboard" | "risk" | "products">).detail;
      if (mode === "dashboard" || mode === "risk" || mode === "products") setViewMode(mode);
    };
    window.addEventListener("dashboard-view", handleDashboardView);
    return () => window.removeEventListener("dashboard-view", handleDashboardView);
  }, [setViewMode]);
  // Right-column collapsible cards (System Info / Storage / Network Traffic).
  // At most MAX_OPEN_CARDS are expanded at once; opening another evicts the
  // longest-ago-opened one. The open set is an ordered list (oldest first)
  // persisted in app settings so the layout survives restarts.
  const persistedOpenCards = appSettings?.app?.dashboardOpenCards;
  const openCards = (persistedOpenCards && persistedOpenCards.length > 0)
    ? persistedOpenCards
    : DEFAULT_OPEN_CARDS;
  const isCardOpen = (id: string) => openCards.includes(id);
  const toggleCard = useCallback((id: string) => {
    const current = (appSettings?.app?.dashboardOpenCards?.length
      ? appSettings.app.dashboardOpenCards
      : DEFAULT_OPEN_CARDS).filter(Boolean);
    let next: string[];
    if (current.includes(id)) {
      next = current.filter((c) => c !== id);          // collapse
    } else if (current.length >= MAX_OPEN_CARDS) {
      next = [...current.slice(1), id];                // evict oldest (front), open new
    } else {
      next = [...current, id];                         // open new
    }
    patchAppSettings({ app: { dashboardOpenCards: next } });
  }, [appSettings?.app?.dashboardOpenCards, patchAppSettings]);
  const [capabilityPending, setCapabilityPending] = useState<"webcam" | "microphone" | null>(null);
  const [internetCut, setInternetCut] = useState(false);
  const [internetPending, setInternetPending] = useState(false);
  const isExpert = visibility.density === "expert";
  const borrowedActive = useBorrowedActive();
  const borrowedHidden = appSettings?.app?.borrowedHidden ?? [];
  // Secret Settings owns both the permanent and Borrowed Mode visibility of
  // these dashboard destinations. `null` preserves the existing default of
  // showing them; only an explicit false is a permanent concealment choice.
  const showRiskMatrix =
    appSettings?.ideal?.identity?.riskMatrixEnabled !== false
    && !(borrowedActive && borrowedHidden.includes("risk-matrix"));
  const showMoreProducts =
    appSettings?.ideal?.identity?.moreProductsEnabled !== false
    && !(borrowedActive && borrowedHidden.includes("more-products"));
  const effectiveViewMode: "dashboard" | "risk" | "products" =
    viewMode === "risk" && showRiskMatrix
      ? "risk"
      : viewMode === "products" && showMoreProducts
        ? "products"
        : "dashboard";

  useEffect(() => {
    if (viewMode !== effectiveViewMode) {
      setViewMode(effectiveViewMode);
    }
  }, [viewMode, effectiveViewMode, setViewMode]);

  // The radar promises live Public IP + DNS readouts. Public IP owns its own
  // probe, but DNS is shared AppContext state and was never requested when a
  // session opened directly on Dashboard, leaving the tile on "checking…"
  // until Network Control happened to mount. Prime that shared state once;
  // refreshNetwork's TTL guard prevents redundant backend work on revisits.
  useEffect(() => {
    void refreshNetwork(true);
  }, [refreshNetwork]);

  // Findings the user has chosen to ignore are hidden from the radar counts,
  // the Needs-Attention list, and Fix Everything (persisted in app settings).
  const ignoredFindingIds = useMemo(
    () => appSettings?.app?.ignoredFindingIds ?? [],
    [appSettings?.app?.ignoredFindingIds]
  );
  // These are optional/panel-gated deps — surfaced only within their own panel,
  // not in the dashboard "Needs Attention" list.
  const missingEngineFindings = useMemo<ScanFinding[]>(
    () => (dependencyStatus ?? [])
      .filter((dep) => dep.installed !== true && !DASHBOARD_HIDDEN_DEPS.has(dep.id))
      .map((dep) => ({
        id: `dependency:${dep.id}`,
        category: "engines",
        label: `${dep.name} not installed`,
        impact: `Required by ${dep.panelId}. Install this engine to unlock the workflow.`,
        severity: "warning",
        safeDefault: true,
      })),
    [dependencyStatus]
  );
  const pendingUpdateFindings = useMemo<ScanFinding[]>(
    () => (appInventory?.pendingUpdates ?? [])
      .filter((update) => (update.id || "").trim().length > 0)
      .map((update) => {
        const name = (update.name || update.id).trim();
        const versionText = update.installedVersion && update.latestVersion
          ? `${update.installedVersion} -> ${update.latestVersion}`
          : "Update available";
        return {
          id: `app-update:${update.id}`,
          category: "updates",
          label: `${name} update available`,
          impact: versionText,
          severity: "warning",
          safeDefault: true,
        };
      }),
    [appInventory?.pendingUpdates]
  );
  // A Pro-sidecar UPGRADE: installed, but a strictly-newer verified build
  // exists. Kept SEPARATE from the not-installed / unverifiable-repair cases
  // (which stay a distinct pro-sidecar finding below) because an upgrade folds
  // into the single combined WinCommander update — Free and Pro ship lockstep.
  const proUpdateAvailable = useMemo(() => {
    if (!hasPaid || !pro.status || pro.manifestError || !pro.status.installed) return false;
    const hashMatchesLatest = !!(
      pro.manifest?.sha256
      && pro.status.local_sha256
      && pro.status.local_sha256.toLowerCase() === pro.manifest.sha256.toLowerCase()
    );
    if (hashMatchesLatest) return false;
    const proCompare = compareVersions(pro.manifest?.version, pro.status.local_version);
    return !!pro.status.local_sha256 && !!pro.status.local_version && proCompare === 1;
  }, [hasPaid, pro.manifest?.sha256, pro.manifest?.version, pro.manifestError, pro.status]);
  const freeUpdatePending =
    updater.phase === "available" || updater.phase === "staged" || updater.phase === "ready";
  // ONE combined "WinCommander update" finding. Free (app updater) and Pro
  // (sidecar) now ship as a single lockstep release, so Fix All / Needs
  // Attention shows a single row instead of a separate Free + Pro row; its Fix
  // opens the combined Free→Pro UpdateFlowDialog. "available" = auto-update OFF,
  // "staged" = downloaded+verified, "ready" = installed, awaiting relaunch.
  const combinedUpdateFinding = useMemo<ScanFinding | null>(() => {
    if (!freeUpdatePending && !proUpdateAvailable) return null;
    return {
      id: "wincommander-update",
      category: "updates",
      label: "WinCommander update available",
      impact: freeUpdatePending && updater.version
        ? `${updater.currentVersion ?? "current"} -> ${updater.version}`
        : "A new version is ready to install",
      severity: "warning",
      safeDefault: true,
    };
  }, [freeUpdatePending, proUpdateAvailable, updater.version, updater.currentVersion]);
  // pro-sidecar now covers ONLY not-installed / unverifiable-repair — an actual
  // Pro version upgrade is represented by combinedUpdateFinding above.
  const proFinding = useMemo<ScanFinding | null>(() => {
    if (!hasPaid || !pro.status || pro.manifestError) return null;
    if (!pro.status.installed) {
      return {
        id: "pro-sidecar",
        category: "updates",
        label: "Pro sidecar not installed",
        impact: "Paid features need the Pro binary on this machine",
        severity: "warning",
        safeDefault: true,
      };
    }
    const hashMatchesLatest = !!(
      pro.manifest?.sha256
      && pro.status.local_sha256
      && pro.status.local_sha256.toLowerCase() === pro.manifest.sha256.toLowerCase()
    );
    if (hashMatchesLatest) return null;
    // Installed but the binary can't be verified against the manifest → repair.
    if (!pro.status.local_version) {
      return {
        id: "pro-sidecar",
        category: "updates",
        label: "Pro sidecar needs repair",
        impact: "The installed Pro binary could not be verified against release metadata",
        severity: "warning",
        safeDefault: true,
      };
    }
    return null;
  }, [hasPaid, pro.manifest?.sha256, pro.manifestError, pro.status]);
  const registryDriftFindings = useMemo<ScanFinding[]>(() => {
    if (!appSettings) return [];
    const level = visibility.density === "expert" ? "advanced" : "standard";
    return getRadarDriftToggles()
      .filter((toggle) => canUse(toggle.tier))
      .filter((toggle) => visibility.isVisible(getToggleVisibility(toggle, visibility.profiles)))
      .map((toggle): ScanFinding | null => {
        const drift = getToggleDrift(appSettings, toggle);
        if (!drift) return null;
        const wording = resolveToggleText(toggle, level);
        return {
          id: `drift:${toggle.id}`,
          category: toggle.radarCategory ?? (toggle.domain === "tweaks" ? "performance" : "privacy"),
          label: drift.targetChecked ? wording.label : `Revert ${wording.label}`,
          impact: drift.targetChecked
            ? "User intent is ON, but Windows reports it OFF. Re-apply the desired setting."
            : "User intent is OFF, but Windows reports it ON. Revert to the desired setting.",
          severity: toggle.radarSeverity ?? "warning",
          safeDefault: toggle.safeDefault,
          drift: true,
          targetChecked: drift.targetChecked,
        };
      })
      .filter((finding): finding is ScanFinding => finding !== null);
  }, [appSettings, canUse, visibility]);
  const allFindings = useMemo(
    () => {
      const seenFindingIds = new Set<string>();
      const seenToggleIds = new Set<string>();
      return [
        ...(radar.report?.findings ?? []),
        ...registryDriftFindings,
        ...missingEngineFindings,
        ...pendingUpdateFindings,
        ...(combinedUpdateFinding ? [combinedUpdateFinding] : []),
        ...(proFinding ? [proFinding] : []),
      ].filter((finding) => {
        if (seenFindingIds.has(finding.id)) return false;
        seenFindingIds.add(finding.id);
        const toggleId = finding.id.startsWith("drift:")
          ? finding.id.slice("drift:".length)
          : finding.id;
        if (getToggleById(toggleId)) {
          if (seenToggleIds.has(toggleId)) return false;
          seenToggleIds.add(toggleId);
        }
        return true;
      });
    },
    [radar.report?.findings, registryDriftFindings, missingEngineFindings, pendingUpdateFindings, combinedUpdateFinding, proFinding]
  );
  const activeFindings = allFindings.filter((f) => {
    if (ignoredFindingIds.includes(f.id)) return false;
    // An app-update item must drop out of Needs Attention / Fix All the instant
    // it's claimed by an in-flight upgrade — whether that upgrade was queued
    // from the Apps panel, the Update-All button, or Fix Everything — so it's
    // never listed (or re-run) a second time. queuedAppUpdateIds is marked
    // synchronously at each queuing site; the running-task flags are the
    // backstop. Anything still pending reappears after the task finishes via
    // refreshSettings re-reading the inventory.
    if (f.id.startsWith("app-update:")) {
      const appId = f.id.slice("app-update:".length);
      if (queuedAppUpdateIds.has(appId) || isAppUpdateTaskRunning || fixAllInProgress) {
        return false;
      }
    }
    return true;
  });
  const driftFindings = useMemo(
    () => registryDriftFindings,
    [registryDriftFindings],
  );
  const hasPendingFixes = radar.phase === "complete" && activeFindings.length > 0;
  // The Map/Risk/Products view toggle hides while the pending-fix (NeedsAttention)
  // list is expanded — so the fixes own the center while you're working through
  // them. The whitelabeled wordmark stays mounted at all times.
  const [needsAttentionExpanded, setNeedsAttentionExpanded] = useState(false);
  const previousActiveFindingCountRef = useRef(0);
  useEffect(() => {
    const previousActiveCount = previousActiveFindingCountRef.current;
    const activeCount = activeFindings.length;
    previousActiveFindingCountRef.current = activeCount;

    if (activeCount === 0 && ignoredFindingIds.length === 0) {
      setNeedsAttentionExpanded(false);
      return;
    }

    if (previousActiveCount === 0 && activeCount > 0) {
      setNeedsAttentionExpanded(true);
    }
  }, [activeFindings.length, ignoredFindingIds.length]);
  const hideCenterChrome = hasPendingFixes && needsAttentionExpanded;

  const naExpandedWithFindings = needsAttentionExpanded && activeFindings.length > 0;
  const updateFindingCount = activeFindings.filter((f) => f.category === "updates").length;
  const pendingAppUpdateCount = activeFindings.filter((f) => f.id.startsWith("app-update:")).length;
  const [busyIds, setBusyIds] = useState<Set<string>>(new Set());
  // Category filter set by clicking a radar node; null = show all.
  const [categoryFilter, setCategoryFilter] = useState<string | null>(null);

  // Map a single finding → an executable op (registry-driven), mirroring the
  // Rust resolution. Returns null when the finding has no actionable command.
  // Special cases:
  //   telemetry-blocklist  → network command (not a toggle)
  //   browser-hardening:*  → browser hardening command
  //   capabilityKey set    → Set-AppCapabilityAccess needs {Capability, Access}
  //   contextMenuShred/Scrub → Rust invoke path, needs patchAppSettings after
  //   suggestions          → companion Disable-SetupCompletionNags
  //   paste-monitor        → settings-only (no toggle/command), flips ideal.privacy.clipboard.pasteMonitorEnabled
  const buildFindingOp = useCallback((f: ScanFinding): { label: string; fn: () => Promise<any> } | null => {
    const wrap = (fn: () => Promise<any>) => async () => {
      const res = await fn();
      if (res && (res as any).error) throw new Error((res as any).error);
      if (res && res.success === false) throw new Error("Operation failed");
      return res;
    };
    const runToggleCommand = async (toggleId: string, targetChecked = true) => {
      const toggle = getToggleById(toggleId);
      if (!toggle) return null;
      if (toggle.capabilityKey) {
        return executeBackendCommand('Set-AppCapabilityAccess', {
          Capability: toggle.capabilityKey,
          Access: targetChecked ? 'Deny' : 'Allow',
        });
      }
      const res = await executeBackendCommand(targetChecked ? toggle.enableCmd : toggle.disableCmd);
      if (targetChecked && toggle.id === 'suggestions') {
        await executeBackendCommand('Disable-SetupCompletionNags');
      }
      return res;
    };
    let fn: (() => Promise<any>) | null = null;
    if (f.id === 'telemetry-blocklist') {
      fn = () => executeBackendCommand('Add-BlocklistToHosts', { BlocklistName: 'telemetry-blocklist' });
    } else if (f.id.startsWith('drift:')) {
      const toggleId = f.id.slice('drift:'.length);
      fn = () => runToggleCommand(toggleId, f.targetChecked ?? false);
    } else if (f.id.startsWith('dependency:')) {
      const depId = f.id.slice('dependency:'.length);
      fn = async () => {
        const res = await executeBackendCommand('Install-Dependency', { Id: depId });
        await refreshDependencies(true);
        return res;
      };
    } else if (f.id.startsWith('app-update:')) {
      const appId = f.id.slice('app-update:'.length);
      fn = async () => {
        const winget = await testWingetInstalled();
        if (!winget.success || winget.data?.status !== "installed") {
          const installRes = await installWinget();
          if (!installRes.success) {
            throw new Error(installRes.error || "Package manager is not available.");
          }
        }
        const res = await upgradeApp(appId);
        if (!res.success) {
          throw new Error(res.error || `Upgrade failed for ${f.label}.`);
        }
        await getAppInventory();
        return res;
      };
    } else if (f.id === 'wincommander-update') {
      // Combined Free+Pro update — always opens the one combined dialog.
      fn = async () => {
        setUpdateFlowOpen(true);
        return { success: true };
      };
    } else if (f.id === 'pro-sidecar') {
      fn = async () => {
        await pro.refresh();
        // pro-sidecar now only means not-installed / needs-repair (a real Pro
        // version upgrade folds into the combined wincommander-update finding),
        // so always route to the standalone Pro installer.
        window.dispatchEvent(new CustomEvent("pro-install-open"));
        return { success: true };
      };
    } else if (f.id.startsWith('browser-hardening:')) {
      const browserName = f.id.slice('browser-hardening:'.length);
      fn = () => executeBackendCommand('Enable-HardenBrowserByName', { Name: browserName });
    } else if (f.id === 'services-profile') {
      fn = async () => {
        const res = await executeBackendCommand('Set-ServicesManual');
        if (res && ((res as any).error || res.success === false)) return res;
        const previous = appSettings?.ideal?.tweaks?.maintenanceRuns?.services;
        await patchAppSettings({
          ideal: { tweaks: { maintenanceRuns: {
            services: { lastRunAt: new Date().toISOString(), runCount: (previous?.runCount ?? 0) + 1 },
          } } },
        });
        return res;
      };
    } else if (f.id === 'disk-cleanup') {
      fn = async () => {
        const res = await invokeDiskCleanup();
        const previous = appSettings?.ideal?.tweaks?.maintenanceRuns?.cleanup;
        await patchAppSettings({
          ideal: { tweaks: { maintenanceRuns: {
            cleanup: { lastRunAt: new Date().toISOString(), runCount: (previous?.runCount ?? 0) + 1 },
          } } },
        });
        return res;
      };
    } else if (f.id === 'paste-monitor') {
      fn = async () => {
        await patchAppSettings({ ideal: { privacy: { clipboard: { pasteMonitorEnabled: true } } } });
        return { success: true };
      };
    } else if (f.id === 'contextMenuShred') {
      fn = async () => {
        await toggleContextMenu(true);
        const actual = await getContextMenuStatus();
        await patchAppSettings({ app: { contextMenuEnabled: actual } });
      };
    } else if (f.id === 'contextMenuScrub') {
      fn = async () => {
        await toggleScrubContextMenu(true);
        const actual = await getScrubContextMenuStatus();
        await patchAppSettings({ app: { scrubContextMenuEnabled: actual } });
      };
    } else {
      const toggle = getToggleById(f.id);
      if (toggle) {
        fn = () => runToggleCommand(toggle.id, f.targetChecked ?? true);
      }
    }
    return fn ? { label: f.label, fn: wrap(fn) } : null;
  }, [toggleContextMenu, getContextMenuStatus, toggleScrubContextMenu, getScrubContextMenuStatus, appSettings?.ideal?.tweaks?.maintenanceRuns?.services, appSettings?.ideal?.tweaks?.maintenanceRuns?.cleanup, patchAppSettings, refreshDependencies, testWingetInstalled, installWinget, getAppInventory, upgradeApp, invokeDiskCleanup, pro, setUpdateFlowOpen]);

  // Run a set of findings through the operation overlay, then re-read state so
  // the radar, score, and toggles update. Used by both Fix-all and per-item Fix.
  // Returns the settle promise so callers (handleFixAll) can track completion
  // directly, instead of only through the TaskStatusContext-derived
  // isFixEverythingRunning (see fixAllInProgress, above).
  const fixFindings = useCallback((targets: ScanFinding[], title: string) => {
    const opSteps = targets
      .map(buildFindingOp)
      .filter((s): s is { label: string; fn: () => Promise<any> } => s !== null);
    if (opSteps.length === 0) return Promise.resolve();
    const ids = targets.map((t) => t.id);
    const appUpdateIds = targets
      .filter((t) => t.id.startsWith('app-update:'))
      .map((t) => t.id.slice('app-update:'.length));
    const shouldRefreshNetwork = ids.includes('telemetry-blocklist');
    const hasAppUpdates = appUpdateIds.length > 0;
    // Fix Everything may combine unrelated repairs with one or more Winget
    // upgrades. Hold the process-wide package lock for the complete batch so
    // another update or install surface cannot start competing package work.
    // The operation still runs its own app-update steps in parallel.
    if (hasAppUpdates && !tryAcquirePackageOperation()) {
      void showWarning("Another package-manager operation is already running.");
      return Promise.resolve();
    }
    // Claim the app-update ids synchronously so Needs Attention / Fix All drop
    // them immediately, and a concurrent Apps-panel "Update All" skips them.
    if (hasAppUpdates) markAppUpdatesQueued(appUpdateIds);
    setBusyIds((prev) => { const n = new Set(prev); ids.forEach((i) => n.add(i)); return n; });
    // Always parallel — privacy toggles are isolated PowerShell processes and
    // the Apps panel already runs winget upgrades in parallel, so bundled
    // app-updates no longer force the whole batch to run one-at-a-time.
    return runOperation(title, opSteps, { mode: 'parallel', accent: 'blue', failFast: false, autoDismissMs: 5000 })
      .then(async () => {
        await refreshSettings();
        if (shouldRefreshNetwork) {
          await refreshNetwork(true, true);
        }
        // Lets the "Fix all" tour step (do-it-yourself) unlock its Next button
        // once the real operation actually finishes — see tour-dashboard.
        if (title === "Fix Everything") {
          window.dispatchEvent(new CustomEvent("tour-fix-all-done"));
        }
      })
      .finally(() => {
        setBusyIds((prev) => { const n = new Set(prev); ids.forEach((i) => n.delete(i)); return n; });
        if (hasAppUpdates) clearAppUpdatesQueued(appUpdateIds);
        if (hasAppUpdates) releasePackageOperation();
      });
  }, [buildFindingOp, refreshSettings, refreshNetwork]);

  // Also broadcast as a window event so Sidebar (a sibling, not a child of
  // this component) can blur in sync without going through TaskStatusContext.
  const handleFixAll = useCallback(() => {
    // Fix Everything is the umbrella — always includes app-update findings so
    // the user gets one operation, one notification. Guard against double-firing
    // when Fix Everything is already running (e.g. rapid double-click).
    if (fixAllInProgress) return;
    setIsFixAllRunning(true);
    void fixFindings(activeFindings, "Fix Everything").finally(() => {
      setIsFixAllRunning(false);
    });
  }, [fixFindings, activeFindings, fixAllInProgress]);
  const handleHealDrift = useCallback(() => fixFindings(driftFindings, "Heal Drift"), [fixFindings, driftFindings]);
  const handleFixOne = useCallback((f: ScanFinding) => {
    if (busyIds.size > 0) return;
    // Don't run individual app-update fixes while any bulk app-update task is active.
    if (f.id.startsWith("app-update:") && (isAppUpdateTaskRunning || fixAllInProgress)) return;
    fixFindings([f], `Fix: ${f.label}`);
  }, [busyIds.size, fixFindings, isAppUpdateTaskRunning, fixAllInProgress]);
  const handleIgnoreFinding = useCallback((f: ScanFinding) => {
    const next = [...new Set([...ignoredFindingIds, f.id])];
    void patchAppSettings({ app: { ignoredFindingIds: next } }).catch(() => {});
  }, [ignoredFindingIds, patchAppSettings]);

  // KT: Use data availability instead of loading flags for the scrambler.
  // State is pre-seeded from cached settings.json (~5ms), so on subsequent runs
  // appSettings is non-null before the dashboard even mounts.
  // On first run (no cache), everything is null → shows scrambler naturally.
  // Only gate on system + hardening (critical for score). Other loading flags
  // (privacy, dashboard, network) resolve silently and don't need to block render.
  const hasCachedData = appSettings !== null;
  const isLoading = !hasCachedData && (loading.system || loading.hardening);
  const finalScore = score.total;

  // Score scrambler while loading
  useEffect(() => {
    if (isLoading) {
      // KT: 500ms → was 150ms (~6.7 React renders/sec on startup); 500ms still scrambles visually with far less state churn
      const interval = setInterval(() => setDisplayScore(Math.floor(Math.random() * 99)), 500);
      return () => clearInterval(interval);
    } else {
      setDisplayScore(finalScore);
    }
  }, [isLoading, finalScore]);

  // Internet kill switch: seed from the live firewall state (authoritative),
  // falling back to the persisted preference until the probe resolves.
  useEffect(() => {
    let cancelled = false;
    invoke<boolean>("internet_kill_switch_get")
      .then((cut) => { if (!cancelled) setInternetCut(cut); })
      .catch(() => { if (!cancelled) setInternetCut(appSettings?.app?.internetKillSwitch === true); });
    return () => { cancelled = true; };
  }, [appSettings?.app?.internetKillSwitch]);

  const handleToggleInternet = useCallback(async (cut: boolean) => {
    setInternetPending(true);
    try {
      const result = await invoke<boolean>("internet_kill_switch_set", { enable: cut });
      setInternetCut(result);
      await patchAppSettings({ app: { internetKillSwitch: result } });
    } catch (e) {
      console.error("Internet kill switch toggle failed:", e);
    } finally {
      setInternetPending(false);
    }
  }, [patchAppSettings]);

  // Calculator lock button — visible only when a Real PIN is configured,
  // the startup-pin gate is not explicitly disabled, AND the session is not
  // borrowed. A coercer in a borrowed session must not see this button and
  // learn that a real PIN exists on this machine.
  const startupPin = appSettings?.ideal?.privacy?.startupPin;
  const calculatorLockArmed = !!startupPin?.realHash && startupPin.enabled !== false && !borrowedActive;
  const handleLockToCalculator = useCallback(() => {
    invoke("lock_to_calculator").catch(() => {});
  }, []);

  const webcamCap = getByPath(appSettings, "current.privacy.appCapabilities.webcam");
  const cameraAvailable = webcamCap !== null && webcamCap !== undefined;
  const cameraBlocked = webcamCap === "Deny";
  const microphoneBlocked = getByPath(appSettings, "current.privacy.appCapabilities.microphone") === "Deny";

  const missingEngineCount = activeFindings.filter((f) => f.category === "engines").length;

  const handleCapabilityToggle = useCallback(async (capability: "webcam" | "microphone", blocked: boolean) => {
    setCapabilityPending(capability);
    try {
      await executeBackendCommand("Set-AppCapabilityAccess", {
        Capability: capability,
        Access: blocked ? "Deny" : "Allow",
      });
      await refreshSettings();
    } finally {
      setCapabilityPending(null);
    }
  }, [refreshSettings]);

  // Ambient backdrop glow tracks the health BAND as a peripheral-vision cue:
  // calm green when strong, amber as it slips, red at risk — driven by the
  // score band colour (status is signalled by status colour, never the brand
  // accent). CSS reads `--dash-glow`, so the tint is theme-correct in both
  // Anduril and Daylight and eases smoothly when the band changes.
  const glowColor = isLoading ? "var(--color-text-muted)" : score.color;

  // Cleanup is intentionally absent here: its recursive scan is started only
  // after explicit Maintenance/Cleanup intent, never by the launch dashboard.
  const fixFooterActions = pendingAppUpdateCount > 0 ? (
    <div className="na-footer-actions">
      {pendingAppUpdateCount > 0 && <AppsUpdateButton compact />}
    </div>
  ) : null;

  return (
    <div
      className={`dashboard-panel dashboard-panel--${effectiveViewMode}${hideCenterChrome ? " dashboard-panel--watermark" : ""}`}
      ref={containerRef}
      style={{ "--dash-glow": glowColor } as React.CSSProperties}
    >
      <div className="dashboard-radar-watermark" aria-hidden="true">{branding.productLabel}</div>
      {calculatorLockArmed && (
        <button
          type="button"
          className="dashboard-lock-btn"
          onClick={handleLockToCalculator}
          title="Lock to calculator mode"
        >
          <Icon icon="lock" size={14} />
          Lock
        </button>
      )}
      <div className="dashboard-scaler" style={{ "--dashboard-scale": windowScale } as React.CSSProperties}>
        <div className={`tech-grid ${score.isArmed ? 'armed' : ''}`} />

        {/* ── Left sidebar: system cards ─────────────────────────────────────
            KT: Was position:absolute — cards didn't push siblings, expanding
            one would overlap the next. Now a real flex-column in the scaler row.
            Collapses to w-0 on non-map views so the center takes full width.
            On small screens, constrains to max-w-[20vw] to preserve center space. */}
        <div className={`dashboard-top-bar transition-all duration-300 ${effectiveViewMode !== "dashboard" ? "w-0 opacity-0 pointer-events-none" : "w-[260px] opacity-100"}`}>
          <RecentDownloadsCard />
          <div data-tour="dashboard-privacy-toggles">
            <PrivacyTogglesCard
              cameraBlocked={cameraBlocked}
              cameraAvailable={cameraAvailable}
              microphoneBlocked={microphoneBlocked}
              capabilityPending={capabilityPending}
              onCapabilityToggle={handleCapabilityToggle}
              internetCut={internetCut}
              internetPending={internetPending}
              onToggleInternet={handleToggleInternet}
            />
          </div>
        </div>

        {/* ── Left: Radar / Risk / Products ─────────────────────── */}
        <div className="dash-left">
          <div className="map-stage">
            <div className="dashboard-view-stage">
              <div className={`dashboard-view-nav dashboard-view-nav--${effectiveViewMode}`}>
                <ViewToggle
                  viewMode={effectiveViewMode}
                  setViewMode={setViewMode}
                  showRiskMatrix={showRiskMatrix}
                  showMoreProducts={showMoreProducts}
                />
              </div>
              <AnimatePresence mode="wait">
              {effectiveViewMode === "risk" ? (
                <motion.div
                  key="risk-matrix"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  // View-switch crossfade: shared cadence via SSOT tokens
                  // (was hardcoded 0.2s; now DURATION_S.normal + EASE.standard).
                  transition={{ duration: DURATION_S.normal, ease: EASE.standard }}
                  className="w-full min-h-0 flex-1 flex items-stretch justify-start p-0 overflow-hidden"
                >
                  <Suspense fallback={<DashboardViewFallback label="Loading risk matrix" />}>
                    <SovereigntyRiskMatrix />
                  </Suspense>
                </motion.div>
              ) : effectiveViewMode === "products" ? (
                <motion.div
                  key="more-products"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  // Same crossfade cadence as the other two views for consistency.
                  transition={{ duration: DURATION_S.normal, ease: EASE.standard }}
                  className="w-full min-h-0 flex-1"
                >
                  <Suspense fallback={<DashboardViewFallback label="Loading products" />}>
                    <MoreProductsView />
                  </Suspense>
                </motion.div>
              ) : (
                <motion.div
                  key="live-radar"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  // Same crossfade cadence as the other two views for consistency.
                  transition={{ duration: DURATION_S.normal, ease: EASE.standard }}
                  className="w-full min-h-0 flex-1 dashboard-radar-wrap"
                >
                  {/* Radar stays vertically centered (flex:1); the fix-actions
                      sit below in their own flex-none row so the Clean /
                      Update-All-Apps footer is never clipped. */}
                  <div className={`radar-center-group${hideCenterChrome ? ' radar-center-group--expanded' : ''}`}>
                  {/* Score count-up + pop is rendered inside SovereigntyRadar's
                      core via <AnimatedNumber> (visible, layout-safe). */}
                  <SovereigntyRadar
                    phase={radar.phase}
                    report={radar.report ? { ...radar.report, findings: activeFindings } : null}
                    score={radar.phase === 'complete' ? finalScore : displayScore}
                    scoreColor={score.color}
                    pendingAppUpdates={updateFindingCount}
                    missingEngineCount={missingEngineCount}
                    optimal={
                      radar.phase === 'complete' &&
                      !!radar.report &&
                      activeFindings.length === 0 &&
                      updateFindingCount === 0
                    }
                    onNodeClick={setCategoryFilter}
                    tourState={activeFindings.length === 0 ? "done" : undefined}
                  />
                  {!hideCenterChrome && (
                    <div className="dashboard-radar-updates">
                      <UpdaterStatus />
                    </div>
                  )}
                  </div>
                  {driftFindings.length > 0 && (
                    <div className="dashboard-drift-oneshot">
                      <button
                        type="button"
                        className="dashboard-heal-drift-btn"
                        onClick={handleHealDrift}
                        disabled={busyIds.size > 0}
                        title="Re-apply all drifted toggle choices"
                        aria-label={`Heal ${driftFindings.length} drifted setting${driftFindings.length === 1 ? "" : "s"}`}
                      >
                        Heal drift
                        <span aria-hidden="true">{driftFindings.length}</span>
                      </button>
                    </div>
                  )}
                  {radar.phase === 'complete' && (
                    <div className="dashboard-fix-actions">
                      <NeedsAttention
                        findings={activeFindings}
                        busyIds={busyIds}
                        onFixOne={handleFixOne}
                        onFixAll={handleFixAll}
                        onIgnore={handleIgnoreFinding}
                        categoryFilter={categoryFilter}
                        onClearFilter={() => setCategoryFilter(null)}
                        expanded={needsAttentionExpanded}
                        onExpandedChange={setNeedsAttentionExpanded}
                        expandedFooter={naExpandedWithFindings ? fixFooterActions : undefined}
                      />
                      {/* When collapsed (or no findings), the cleanup chip +
                          Update All Apps sit below the radar instead. */}
                      {!naExpandedWithFindings && fixFooterActions}
                    </div>
                  )}
                </motion.div>
              )}
              </AnimatePresence>
            </div>
          </div>
        </div>

        {/* ── Right sidebar: hardware / storage / network cards ──────────────
            KT: Was position:absolute (same problem as left sidebar above).
            Now a real flex-column sibling; collapses to w-0 on non-map views.
            On small screens, constrains to max-w-[22vw] to preserve center space. */}
        <div className={`dashboard-right-bar transition-all duration-300 ${effectiveViewMode !== "dashboard" ? "w-0 opacity-0 pointer-events-none" : "w-[320px] opacity-100"}`}>
          <div data-tour="dashboard-hardware-specs">
            <HardwareSpecsCard
              isLoading={isLoading}
              systemInfo={dashboardSystemInfo}
              metricsStatus={liveMetricsStatus}
              expanded={isCardOpen("system")}
              onToggle={() => toggleCard("system")}
            />
          </div>
          {isExpert && (
            <StorageOverviewCard
              isLoading={isLoading}
              systemInfo={dashboardSystemInfo}
              expanded={isCardOpen("storage")}
              onToggle={() => toggleCard("storage")}
            />
          )}
          <TierGate tier="paid" featureLabel="Network Traffic Monitor">
            <NetworkTrafficCard
              expanded={isCardOpen("network")}
              onToggle={() => toggleCard("network")}
            />
          </TierGate>
        </div>
      </div>

      {/* Opened by the combined "wincommander-update" finding's Fix action — a
          local instance mirroring VersionManagementCard's (Settings) pattern
          rather than the app-level singleton in App.tsx, which owns only the
          once-per-session startup auto-trigger. updateAvailable is always true
          here since this dialog only opens when combinedUpdateFinding already
          exists (Free phase available/staged/ready or a Pro upgrade). */}
      <UpdateFlowDialog
        isOpen={updateFlowOpen}
        onClose={() => setUpdateFlowOpen(false)}
        hasPaid={hasPaid}
        updateAvailable
      />
    </div>
  );
}

function DashboardViewFallback({ label }: { label: string }) {
  return (
    <div className="h-full w-full flex items-center justify-center text-xs text-[var(--color-text-muted)]" role="status">
      {label}
    </div>
  );
}
