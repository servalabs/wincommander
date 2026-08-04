import { Card, Button, Checkbox, InputGroup, Icon, Spinner } from "@/components/ui/bp";
import { useMemo, useState, useEffect, useCallback, useRef, type ReactNode } from "react";
import { motion } from "framer-motion";
import useBackend from "../../../hooks/useBackend";
import { useAppState } from "../../../context/AppContext";
import { beginOperation, runOperation } from "../../../context/OperationContext";
import { claimFreeAppUpdates, clearAppUpdatesQueued, isAppUpdateQueued } from "../../../lib/appUpdateQueue";
import { releasePackageOperation, tryAcquirePackageOperation } from "../../../lib/packageOperationLock";
import { showWarning, showError, showSuccess } from "../../../utils/toast";
import AppIcon from "./AppIcon";
import { cn } from "../../../lib/utils";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../../components/ui/tabs";
// staggerDelay caps per-item delay so large lists never animate for seconds.
import { staggerDelay } from "../../../components/shared/AnimatedList";
import { DURATION_S, EASE } from "../../../components/shared/motion";
import SuccessFill from "../../../components/shared/SuccessFill";
import './AppInstallerPanel.css';


declare global {
  interface Window {
    __pendingAppInstall?: string[];
    __pendingAppsInstallView?: "updates";
  }
}

interface AppItem {
  id: string;
  name: string;
  description: string;
  category: string;
  installed?: boolean;
  updateAvailable?: boolean;
  version?: string;
  availableVersion?: string;
  iconData?: string | null;
}

// LEARNING: UpgradeItem is the local UI shape for "other updates" (non-manifest apps with
// pending upgrades). Derived from AppInventorySnapshot.pendingUpdates.
interface UpgradeItem {
  id: string;
  name?: string;
  version?: string;
  availableVersion?: string;
  source?: string;
  iconData?: string | null;
}

const CATEGORY_TABS = [
  { id: "all", label: "All" },
  { id: "basic", label: "Basic" },
  { id: "power-user", label: "Advanced" },
  { id: "privacy", label: "Privacy" },
  { id: "developer", label: "Developer" },
  { id: "system-info", label: "System" },
] as const;

function mapCategoryToTab(category: string): string {
  const normalized = category.trim().toLowerCase();

  if (["bench-mon", "bench", "monitor", "monitoring", "system-info", "system", "repair"].includes(normalized)) {
    return "system-info";
  }
  if (["privacy"].includes(normalized)) {
    return "privacy";
  }
  if (["dev", "developer"].includes(normalized)) {
    return "developer";
  }
  if (["power"].includes(normalized)) {
    return "power-user";
  }
  if (["base", "kid", "misc", "teams"].includes(normalized)) {
    return "basic";
  }

  if (normalized.includes("privacy")) return "privacy";
  if (normalized.includes("dev")) return "developer";
  if (normalized.includes("power")) return "power-user";
  if (normalized.includes("bench") || normalized.includes("monitor") || normalized.includes("repair") || normalized.includes("system")) {
    return "system-info";
  }

  return "basic";
}

function AppInstallerPanel({ updatesTools }: { updatesTools?: ReactNode }) {
  const { appInventory, runAppInventoryScan, loading: contextLoading, patchAppSettings, forceRefreshDeps } = useAppState();
  const [selectedCategory, setSelectedCategory] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");
  const appsGridRef = useRef<HTMLDivElement>(null);
  const [selectedApps, setSelectedApps] = useState<Set<string>>(new Set());
  const [installedApps, setInstalledApps] = useState<Set<string>>(new Set());
  const [updateAvailableApps, setUpdateAvailableApps] = useState<Set<string>>(new Set());
  const [otherUpgrades, setOtherUpgrades] = useState<UpgradeItem[]>([]);
  // KT: Per-id install state. The previous single boolean toggled `loading` on
  // every install button at once — clicking download on one card put every
  // other card's button into the loading state too (and BlueprintJS's
  // .bp5-loading overlay rendered as a visible dark box in dark mode).
  const [installingIds, setInstallingIds] = useState<Set<string>>(new Set());
  const installing = installingIds.size > 0;
  // Ids that finished installing THIS session — drives a one-shot wc-app-pop
  // on the row. Cleared shortly after so the pop fires exactly once.
  const [justInstalledIds, setJustInstalledIds] = useState<Set<string>>(new Set());
  const [apps, setApps] = useState<AppItem[]>([]);
  const [appsLoading, setAppsLoading] = useState(!appInventory);
  const [inventoryScanPending, setInventoryScanPending] = useState(false);
  const inventoryScanRequestedRef = useRef(false);
  const [wingetStatus, setWingetStatus] = useState<"checking" | "installed" | "not-installed" | "installing" | "failed">("checking");
  const [upgradingApp, setUpgradingApp] = useState<string | null>(null);
  const [localLoadingMap, setLocalLoadingMap] = useState<Record<string, boolean>>({});
  // Inner sub-tabs for the catalog grid — replaces the old collapsible
  // "UPDATES AVAILABLE" / "INSTALLED (N)" dividers now that each state is its
  // own tab (Radix mount/unmount stands in for expand/collapse).
  const [installerView, setInstallerView] = useState<"not-installed" | "updates" | "installed">("not-installed");
  // Default to false so the REMOVE buttons stay hidden until detection
  // confirms the app is actually installed. Defaulting to true caused a
  // visible flicker on first paint where the buttons appeared, then
  // disappeared when the testEdgeInstalled / testOneDriveInstalled
  // probes came back negative.
  const [edgeInstalled, setEdgeInstalled] = useState(false);
  const [oneDriveInstalled, setOneDriveInstalled] = useState(false);
  const [teamsInstalled, setTeamsInstalled] = useState(false);

  const {
    installWingetApps,
    upgradeApp,
    // LEGACY (commented for visibility/collaboration): bulk updater replaced by per-package sequential queue.
    // upgradeAllApps,
    getAppInventory,
    testWingetInstalled,
    installWinget,
    removeEdge,
    removeOneDrive,
    removeTeams,
    testEdgeInstalled,
    testOneDriveInstalled,
    getTeamsStatus,
    error
  } = useBackend();

  useEffect(() => {
    const checkRemovalStatus = async () => {
      // KT: Parallelized — Edge, OneDrive, and Teams checks are independent.
      const [edge, oneDrive, teams] = await Promise.all([testEdgeInstalled(), testOneDriveInstalled(), getTeamsStatus()]);
      // Unknown / probe failure -> stay hidden. Showing a removal button
      // for an app that may not be installed is worse than missing one
      // for an app that is.
      if (edge.success) setEdgeInstalled(edge.data?.installed ?? false);
      if (oneDrive.success) setOneDriveInstalled(oneDrive.data?.installed ?? false);
      if (teams.success) setTeamsInstalled(teams.data?.installed ?? false);
    };
    checkRemovalStatus();
  }, [testEdgeInstalled, testOneDriveInstalled, getTeamsStatus]);

  const handleUpdateAll = async () => {
    // LEGACY IMPLEMENTATION (commented for visibility/collaboration):
    // setLocalLoadingMap(prev => ({ ...prev, "updateAll": true }));
    // try {
    //   await runOperation("Update All Apps", [
    //     {
    //       label: "Checking package manager...",
    //       fn: async () => {
    //         const winget = await testWingetInstalled();
    //         if (!winget.success || winget.data?.status !== "installed") {
    //           await installWinget();
    //         }
    //       }
    //     },
    //     {
    //       label: "Upgrading all applications...",
    //       fn: async () => {
    //         const res = await upgradeAllApps();
    //         if (!res.success) throw new Error(res.error || "Upgrade all apps failed.");
    //       }
    //     },
    //     {
    //       label: "Refreshing app inventory...",
    //       fn: async () => {
    //         await runAppInventoryScan(true);
    //       }
    //     }
    //   ], { mode: 'sequential', failFast: false, accent: 'blue' });
    // } catch {
    //   // error shown in status bar by runOperation
    // } finally {
    //   setLocalLoadingMap(prev => ({ ...prev, "updateAll": false }));
    // }

    if (!tryAcquirePackageOperation()) {
      showWarning("Another package-manager operation is already running.");
      return;
    }
    setLocalLoadingMap(prev => ({ ...prev, "updateAll": true }));
    // Claim the currently-known pending ids synchronously so the dashboard's
    // Fix All / Needs Attention drops them immediately.
    const mine = claimFreeAppUpdates((appInventory?.pendingUpdates ?? []).map((u) => u.id || ""));
    try {
      // ── Pre-flight (sequential) — package manager must be present
      // and inventory must be enumerated before we can spawn workers.
      const winget = await testWingetInstalled();
      if (!winget.success || winget.data?.status !== "installed") {
        const installRes = await installWinget();
        if (!installRes.success) {
          throw new Error(installRes.error || "Package manager is not available.");
        }
      }

      const inventoryRes = await getAppInventory();
      if (!inventoryRes.success || !inventoryRes.data) {
        throw new Error(inventoryRes.error || "Failed to build package upgrade queue.");
      }

      const packageQueue = new Map<string, string>();
      (inventoryRes.data.pendingUpdates ?? []).forEach((item) => {
        const id = (item.id || "").trim();
        if (!id || packageQueue.has(id)) return;
        // Skip ids already claimed by ANOTHER in-flight upgrade.
        if (isAppUpdateQueued(id) && !mine.includes(id)) return;
        packageQueue.set(id, (item.name || "").trim() || id);
      });

      const queueEntries = Array.from(packageQueue.entries());
      mine.push(...claimFreeAppUpdates(queueEntries.map(([id]) => id)));

      if (queueEntries.length === 0) {
        await runOperation("Update packages", [
          { label: "No pending package upgrades found", fn: async () => { } }
        ], { mode: 'sequential', accent: 'blue' });
        return;
      }

      // ── Upgrade steps run in PARALLEL. winget's internal store lock
      // still serializes on the OS side, but concurrent `winget upgrade`
      // invocations queue cooperatively and the operation bar shows all
      // rows progressing instead of one-at-a-time.
      const steps = queueEntries.map(([appId, name]) => ({
        label: `Upgrading ${name}`,
        fn: async () => {
          const res = await upgradeApp(appId);
          if (!res.success) throw new Error(res.error || `Upgrade failed for ${name}.`);
        }
      }));

      await runOperation(
        `Update ${queueEntries.length} Package${queueEntries.length === 1 ? "" : "s"}`,
        steps,
        { mode: 'parallel', failFast: false, accent: 'blue' }
      );

      // Single inventory refresh after all parallel upgrades finish.
      await runAppInventoryScan(true);
    } catch {
      // error shown in status bar by runOperation
    } finally {
      setLocalLoadingMap(prev => ({ ...prev, "updateAll": false }));
      clearAppUpdatesQueued(mine);
      releasePackageOperation();
    }
  };

  const updatePackageQueue = useCallback(async (
    entries: Array<[string, string]>,
    title: string,
    loadingKey: string,
    afterDone?: () => void
  ) => {
    if (!tryAcquirePackageOperation()) {
      showWarning("Another package-manager operation is already running.");
      return;
    }
    setLocalLoadingMap(prev => ({ ...prev, [loadingKey]: true }));
    const mine = claimFreeAppUpdates(entries.map(([id]) => id));
    // Only upgrade packages this call actually claimed — any id already in
    // flight from another update job (Update All / per-card) is skipped so the
    // same package can't be upgraded twice concurrently.
    const myEntries = entries.filter(([id]) => mine.includes(id));
    try {
      if (myEntries.length === 0) {
        showWarning("Those packages are already being updated.");
        return;
      }
      // Pre-flight outside the operation panel — package manager must be
      // present before any upgrade workers run.
      const winget = await testWingetInstalled();
      if (!winget.success || winget.data?.status !== "installed") {
        const installRes = await installWinget();
        if (!installRes.success) {
          throw new Error(installRes.error || "Package manager is not available.");
        }
      }

      // Upgrade steps run in parallel — one row per package in the
      // operation panel, all progressing concurrently.
      const steps = myEntries.map(([appId, name]) => ({
        label: `Upgrading ${name}`,
        fn: async () => {
          const res = await upgradeApp(appId);
          if (!res.success) throw new Error(res.error || `Upgrade failed for ${name}.`);
        }
      }));

      await runOperation(title, steps, { mode: 'parallel', failFast: false, accent: 'blue' });

      // Single inventory refresh after all parallel upgrades finish.
      await runAppInventoryScan(true);
      afterDone?.();
    } catch {
      // error shown in status bar by runOperation
    } finally {
      setLocalLoadingMap(prev => ({ ...prev, [loadingKey]: false }));
      clearAppUpdatesQueued(mine);
      releasePackageOperation();
    }
  }, [installWinget, runAppInventoryScan, testWingetInstalled, upgradeApp]);

  const runRemoval = async (key: string, fn: () => Promise<any>) => {
    const name = key === "removeEdge" ? "Edge" : key === "removeTeams" ? "Teams" : "OneDrive";
    setLocalLoadingMap(prev => ({ ...prev, [key]: true }));
    try {
      await runOperation(`Remove ${name}`, [
        {
          label: `Removing ${name}...`,
          fn: async () => {
            const res = await fn();
            if (!res.success) throw new Error(`Failed to remove ${name}.`);
          }
        }
      ], { mode: 'sequential', accent: 'neutral' });
      if (key === "removeEdge") {
        setEdgeInstalled(false);
        patchAppSettings({ ideal: { apps: { edgeRemoved: true } } }).catch(() => { });
      }
      if (key === "removeOneDrive") {
        setOneDriveInstalled(false);
        patchAppSettings({ ideal: { apps: { onedriveRemoved: true } } }).catch(() => { });
      }
      if (key === "removeTeams") {
        setTeamsInstalled(false);
        patchAppSettings({ ideal: { apps: { teamsRemoved: true } } }).catch(() => { });
      }
    } catch {
      // error shown in status bar by runOperation
    } finally {
      setLocalLoadingMap(prev => ({ ...prev, [key]: false }));
    }
  };

  const installedManifestCount = installedApps.size;
  // Count every pending update, not just manifest apps — non-manifest ("other")
  // packages returned by the inventory scan are also visible in the list below.
  const outdatedInstalledCount = updateAvailableApps.size + otherUpgrades.length;
  const vulnerabilityBadge = useMemo(() => {
    if (installedManifestCount === 0) {
      return {
        tone: "is-neutral",
        text: "NO INSTALLED APPS SCANNED",
        detail: "Scan inventory to assess updates",
      };
    }

    // outdatedInstalledCount also counts pending updates for non-manifest
    // ("other") packages, which aren't part of installedManifestCount, so the
    // raw ratio can exceed 100% — clamp it to a sane percentage.
    const vulnerabilityScore = Math.min(100, Math.round((outdatedInstalledCount / installedManifestCount) * 100));
    if (outdatedInstalledCount === 0) {
      return {
        tone: "is-safe",
        text: "ALL APPS UP TO DATE",
        detail: "0 apps need updates",
      };
    }

    return {
      tone: vulnerabilityScore > 30 ? "is-risk" : "is-warning",
      text: `${outdatedInstalledCount} APP${outdatedInstalledCount === 1 ? "" : "S"} NEED UPDATES`,
      detail: `${vulnerabilityScore}% vulnerability score`,
    };
  }, [installedManifestCount, outdatedInstalledCount]);

  // Derive local state from appInventory (context) whenever it changes
  // LEARNING: appInventory is the single source of truth from Get-AppInventory.
  // It replaces the old contextAppStatus / contextUpgradeList / appManifest / installedApps.
  useEffect(() => {
    if (!appInventory) return;

    // Manifest apps → AppItem[]
    const manifestApps: AppItem[] = (appInventory.manifestApps || []).map(app => ({
      id: app.id,
      name: app.name,
      description: app.description || '',
      category: app.category || '',
      installed: app.installed,
      updateAvailable: app.updateAvailable,
      version: app.installedVersion ?? undefined,
      availableVersion: app.latestVersion ?? undefined,
      iconData: app.iconData ?? null,
    }));
    setApps(manifestApps);
    setInstalledApps(new Set(manifestApps.filter(a => a.installed).map(a => a.id)));
    setUpdateAvailableApps(new Set(manifestApps.filter(a => a.updateAvailable).map(a => a.id)));

    // Other upgrades = pending updates for non-manifest apps.
    // winget can report the same package id more than once (e.g. multiple
    // installed versions of a runtime), so dedupe by id to keep React keys
    // unique and avoid "two children with the same key" warnings.
    const manifestIds = new Set(manifestApps.map(a => a.id));
    const seenOther = new Set<string>();
    const other = (appInventory.pendingUpdates || [])
      .filter(u => u.id && !manifestIds.has(u.id))
      .filter(u => {
        if (seenOther.has(u.id)) return false;
        seenOther.add(u.id);
        return true;
      })
      .map(u => ({
        id: u.id,
        name: u.name ?? undefined,
        version: u.installedVersion ?? undefined,
        availableVersion: u.latestVersion ?? undefined,
        source: u.source ?? undefined,
        iconData: u.iconData ?? null,
      }));
    setOtherUpgrades(other);

    setAppsLoading(false);
  }, [appInventory]);

  useEffect(() => {
    // Check winget status on mount
    const checkWinget = async () => {
      const response = await testWingetInstalled();
      if (response.success && response.data) {
        setWingetStatus(response.data.status);
      } else {
        setWingetStatus("failed");
      }
    };

    // If appInventory hasn't arrived yet (context hasn't scanned yet),
    // trigger a scan. The useEffect above will populate local state when it lands.
    if (!appInventory && !inventoryScanRequestedRef.current) {
      inventoryScanRequestedRef.current = true;
      setInventoryScanPending(true);
      runAppInventoryScan(true).finally(() => setInventoryScanPending(false));
    }
    checkWinget();
  }, [testWingetInstalled, appInventory, runAppInventoryScan]);

  useEffect(() => {
    const openUpdates = () => {
      if (window.__pendingAppsInstallView !== "updates") return;
      window.__pendingAppsInstallView = undefined;
      setInstallerView("updates");
    };
    openUpdates();
    window.addEventListener("apps-open-updates-tab", openUpdates);
    return () => window.removeEventListener("apps-open-updates-tab", openUpdates);
  }, []);

  const handleRefreshAll = useCallback(async () => {
    if (inventoryScanPending || contextLoading?.apps) return;
    setInventoryScanPending(true);
    try {
      await runAppInventoryScan(false);
      const w = await testWingetInstalled();
      if (w.success && w.data) setWingetStatus(w.data.status);
    } finally {
      setInventoryScanPending(false);
    }
  }, [contextLoading?.apps, inventoryScanPending, runAppInventoryScan, testWingetInstalled]);

  // Toast notifications for winget status and backend errors (replaces inline callouts)
  useEffect(() => {
    if (wingetStatus === 'not-installed') {
      showWarning('Package manager not found — will be installed automatically on first run.');
    } else if (wingetStatus === 'failed') {
      showError('Package manager install failed. Run as Administrator and try again.');
    }
  }, [wingetStatus]);

  useEffect(() => {
    if (error) showError(error);
  }, [error]);

  // Auto-scroll to results grid when user types a search query
  useEffect(() => {
    if (searchQuery.trim() && appsGridRef.current) {
      appsGridRef.current.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  }, [searchQuery]);

  const filteredApps = useMemo(() => {
    return apps
      .filter(app => {
        const isSearching = searchQuery.trim() !== "";
        const mappedCategory = mapCategoryToTab(app.category);
        const categoryMatches = isSearching || selectedCategory === "all" || mappedCategory === selectedCategory;
        const matchesSearch = !isSearching ||
          app.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
          app.description.toLowerCase().includes(searchQuery.toLowerCase()) ||
          app.id.toLowerCase().includes(searchQuery.toLowerCase());
        return categoryMatches && matchesSearch;
      })
      .sort((a, b) => {
        // Priority: not installed (0) > updates (1) > installed (2)
        const aInstalled = installedApps.has(a.id);
        const bInstalled = installedApps.has(b.id);
        const aUpdate = updateAvailableApps.has(a.id);
        const bUpdate = updateAvailableApps.has(b.id);
        const aPriority = !aInstalled ? 0 : aUpdate ? 1 : 2;
        const bPriority = !bInstalled ? 0 : bUpdate ? 1 : 2;
        if (aPriority !== bPriority) return aPriority - bPriority;

        // In 'all' view, move developer/system utility apps lower within each group.
        if (selectedCategory === "all") {
          const aMapped = mapCategoryToTab(a.category);
          const bMapped = mapCategoryToTab(b.category);
          const aSpecial = aMapped === "developer" || aMapped === "system-info";
          const bSpecial = bMapped === "developer" || bMapped === "system-info";
          if (aSpecial && !bSpecial) return 1;
          if (!aSpecial && bSpecial) return -1;
        }

        return a.name.localeCompare(b.name);
      });
  }, [apps, selectedCategory, searchQuery, updateAvailableApps, installedApps]);

  const filteredOtherUpgrades = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    if (!query) return otherUpgrades;
    return otherUpgrades.filter(u =>
      (u.name || u.id).toLowerCase().includes(query) ||
      u.id.toLowerCase().includes(query)
    );
  }, [otherUpgrades, searchQuery]);

  const toggleApp = useCallback((appId: string) => {
    setSelectedApps((prev) => {
      const next = new Set(prev);
      if (next.has(appId)) {
        next.delete(appId);
      } else {
        next.add(appId);
      }
      return next;
    });
  }, []);

  const selectAllInCategory = () => {
    setSelectedApps((prev) => {
      const next = new Set(prev);
      filteredApps.forEach(app => {
        if (!installedApps.has(app.id) || updateAvailableApps.has(app.id)) {
          next.add(app.id);
        }
      });
      filteredOtherUpgrades.forEach(item => next.add(item.id));
      return next;
    });
  };

  const clearSelection = () => {
    setSelectedApps(new Set());
  };

  const ensureWinget = useCallback(async () => {
    if (wingetStatus === "installed") {
      return true;
    }
    setWingetStatus("installing");
    const response = await installWinget();
    if (response.success) {
      setWingetStatus("installed");
      return true;
    }
    setWingetStatus("failed");
    return false;
  }, [installWinget, wingetStatus]);

  // ── Install queue ─────────────────────────────────────────────────
  // One worker owns the package lock for as long as anything is left to
  // install, and drains a shared pending list. Asking to install more while a
  // run is in flight APPENDS to that same run and the same task row — there is
  // no separate "schedule" step to press (2026-07-26 fix: the old FIFO batch
  // queue needed an explicit SCHEDULE click, and a single-card install during a
  // run was refused outright by the package lock, so the second app silently
  // never installed).
  //
  // coveredIds is the SYNCHRONOUS source of truth for "the running worker
  // already has this id". installingIds is state and two clicks landing in the
  // same tick would both read its pre-click value, enqueueing the app twice.
  const coveredIdsRef = useRef<Set<string>>(new Set());
  const pendingInstallRef = useRef<string[]>([]);
  const installWorkerRef = useRef(false);

  const installApps = useCallback(async (appIds: string[]) => {
    const uniqueIds = Array.from(new Set(appIds))
      .filter(id => !installedApps.has(id) && !coveredIdsRef.current.has(id));
    if (uniqueIds.length === 0) {
      showWarning("Those apps are already installed, or already in the running install.");
      return;
    }

    uniqueIds.forEach(id => coveredIdsRef.current.add(id));
    pendingInstallRef.current.push(...uniqueIds);
    setSelectedApps(new Set());
    setInstallingIds(prev => {
      const next = new Set(prev);
      uniqueIds.forEach(id => next.add(id));
      return next;
    });

    // A run is already in flight — its drain loop picks these up next.
    if (installWorkerRef.current) {
      showSuccess(`Added ${uniqueIds.length} app${uniqueIds.length === 1 ? "" : "s"} to the running install.`);
      return;
    }

    // A DIFFERENT package-manager surface (Update All / Update Selected) holds
    // the lock. That work isn't ours to append to, so this one really does have
    // to wait — roll the enqueue back rather than leaving ids stuck "installing".
    if (!tryAcquirePackageOperation()) {
      showWarning("Another package-manager operation is already running.");
      uniqueIds.forEach(id => coveredIdsRef.current.delete(id));
      pendingInstallRef.current = pendingInstallRef.current.filter(id => !uniqueIds.includes(id));
      setInstallingIds(prev => {
        const next = new Set(prev);
        uniqueIds.forEach(id => next.delete(id));
        return next;
      });
      return;
    }

    installWorkerRef.current = true;
    const op = beginOperation("Installing apps", { accent: 'blue' });
    try {
      // ── Pre-flight (sequential) — package manager must be present
      // before we can spawn any parallel install workers.
      const ready = await ensureWinget();
      if (!ready) throw new Error("Package manager is not available.");

      // ── Install steps run in PARALLEL. winget's internal store lock
      // still serializes on the OS side, but multiple concurrent
      // `winget install --id X` invocations queue cooperatively and
      // the operation bar shows all rows progressing at once instead
      // of one-at-a-time. Per-id loading flag flips off as each app
      // finishes so its card stops spinning while siblings continue.
      // The loop re-checks the pending list after every batch, so apps added
      // mid-run are installed by this same worker under this same row.
      while (pendingInstallRef.current.length > 0) {
        const batch = pendingInstallRef.current.splice(0);
        await op.add(batch.map(id => ({
          label: `Installing ${apps.find(a => a.id === id)?.name || id}`,
          fn: async () => {
            const response = await installWingetApps([id]);
            if (!response.success) throw new Error(response.error || `Install failed.`);
            // Move a successful install immediately. Waiting for the full
            // inventory refresh made the same app remain in the install grid
            // long enough to invite duplicate clicks and look duplicated.
            setInstalledApps(prev => new Set(prev).add(id));
            setUpdateAvailableApps(prev => {
              const next = new Set(prev);
              next.delete(id);
              return next;
            });
            setSelectedApps(prev => {
              const next = new Set(prev);
              next.delete(id);
              return next;
            });
            setApps(prev => prev.map(app => app.id === id
              ? { ...app, installed: true, updateAvailable: false }
              : app));
            if (uniqueIds.length === 1) setInstallerView("installed");
            setInstallingIds(prev => {
              const next = new Set(prev);
              next.delete(id);
              return next;
            });
            // One-shot "just installed" pop on this row. Cleared after the
            // animation window so re-renders don't replay it.
            setJustInstalledIds(prev => new Set(prev).add(id));
            setTimeout(() => {
              setJustInstalledIds(prev => {
                if (!prev.has(id)) return prev;
                const next = new Set(prev);
                next.delete(id);
                return next;
              });
            }, 600);
          },
        })));
      }

      // Refresh both the packages list and the engine dependency status so the
      // Engines section reflects newly installed CLI tools (e.g. es.exe for search).
      await Promise.all([runAppInventoryScan(true), forceRefreshDeps()]);
    } catch (err) {
      // Pre-flight failures never reached a task row under the old code, so
      // "winget is missing" failed completely silently. Per-step failures are
      // still reported by the row itself.
      showError(err instanceof Error ? err.message : "Install failed.");
    } finally {
      op.finish();
      installWorkerRef.current = false;
      pendingInstallRef.current = [];
      coveredIdsRef.current.clear();
      // Clears any id still marked installing because its step threw before
      // the per-step delete ran.
      setInstallingIds(new Set());
      releasePackageOperation();
    }
  }, [ensureWinget, installWingetApps, installedApps, runAppInventoryScan, forceRefreshDeps, apps]);

  const handleInstall = async () => {
    await installApps(Array.from(selectedApps).filter(id => !installedApps.has(id)));
  };

  const handleUpgradeSingle = async (appId: string, e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    if (!tryAcquirePackageOperation()) {
      showWarning("Another package-manager operation is already running.");
      return;
    }
    const app = apps.find(a => a.id === appId);
    const other = otherUpgrades.find(u => u.id === appId);
    const friendlyName = app?.name || other?.name || appId;
    const mine = claimFreeAppUpdates([appId]);
    // Already being updated by another job (Update All / Update Selected) —
    // don't enqueue a duplicate upgrade for the same package.
    if (mine.length === 0) {
      showWarning(`${friendlyName} is already being updated.`);
      releasePackageOperation();
      return;
    }
    setUpgradingApp(appId);
    try {
      await runOperation(`Upgrade ${friendlyName}`, [
        {
          label: "Checking package manager...",
          fn: async () => {
            const ready = await ensureWinget();
            if (!ready) throw new Error("Package manager is not available.");
          }
        },
        {
          label: `Upgrading ${friendlyName}...`,
          fn: async () => {
            const response = await upgradeApp(appId);
            if (!response.success) throw new Error(response.error || `Upgrade failed for ${friendlyName}.`);
          }
        },
        {
          label: "Refreshing app inventory...",
          fn: async () => {
            await runAppInventoryScan(true);
          }
        }
      ], { mode: 'sequential', failFast: false, accent: 'blue' });
    } catch {
      // error shown in status bar by runOperation
    } finally {
      setUpgradingApp(null);
      clearAppUpdatesQueued(mine);
      releasePackageOperation();
    }
  };

  const selectedUpdateIds = Array.from(selectedApps).filter(id =>
    updateAvailableApps.has(id) || otherUpgrades.some(item => item.id === id)
  );
  const otherUpgradeIds = new Set(otherUpgrades.map(u => u.id));
  const selectedInstallIds = Array.from(selectedApps).filter(id => !installedApps.has(id) && !otherUpgradeIds.has(id));
  const handleUpdateSelected = () => {
    const entries = selectedUpdateIds.map(id => {
      const app = apps.find(item => item.id === id);
      const other = otherUpgrades.find(item => item.id === id);
      return [id, app?.name || other?.name || id] as [string, string];
    });
    if (entries.length === 0) return;
    void updatePackageQueue(
      entries,
      `Update ${entries.length} Selected Package${entries.length === 1 ? "" : "s"}`,
      "updateSelected",
      () => {
        setSelectedApps(prev => {
          const next = new Set(prev);
          entries.forEach(([id]) => next.delete(id));
          return next;
        });
      }
    );
  };

  useEffect(() => {
    const pending = window.__pendingAppInstall;
    if (pending && pending.length > 0 && !appsLoading) {
      window.__pendingAppInstall = undefined;
      installApps(pending);
    }
  }, [appsLoading, installApps]);

  useEffect(() => {
    const handleAppsInstallMissing = (event: Event) => {
      const detail = (event as CustomEvent<{ appIds?: string[] }>).detail;
      const ids = detail?.appIds || window.__pendingAppInstall || [];
      if (ids.length > 0) {
        if (appsLoading) {
          window.__pendingAppInstall = ids;
          return;
        }
        window.__pendingAppInstall = undefined;
        installApps(ids);
      }
    };

    window.addEventListener("apps-install-missing", handleAppsInstallMissing as EventListener);
    return () => {
      window.removeEventListener("apps-install-missing", handleAppsInstallMissing as EventListener);
    };
  }, [appsLoading, installApps]);

  const notInstalledApps = filteredApps.filter(app => !installedApps.has(app.id));
  const updateApps = filteredApps.filter(app => installedApps.has(app.id) && updateAvailableApps.has(app.id));
  const noActionApps = filteredApps.filter(app => installedApps.has(app.id) && !updateAvailableApps.has(app.id));

  const renderApp = (app: AppItem, showCheckbox = true) => {
    const canSelect = showCheckbox && (!installedApps.has(app.id) || updateAvailableApps.has(app.id));
    return (
    <div
      key={app.id}
      className={cn(
        "app-card",
        selectedApps.has(app.id) && "selected",
        installedApps.has(app.id) && "installed",
        updateAvailableApps.has(app.id) && "update-available",
        justInstalledIds.has(app.id) && "wc-app-pop"
      )}
      onClick={() => canSelect && toggleApp(app.id)}
      role={canSelect ? "button" : undefined}
      tabIndex={canSelect ? 0 : undefined}
      aria-pressed={canSelect ? selectedApps.has(app.id) : undefined}
      onKeyDown={(e) => {
        if (!canSelect) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          toggleApp(app.id);
        }
      }}
    >
      {/* Green-sweep confirmation when an app finishes installing this
          session (justInstalledIds clears after 600ms). Non-erase
          completion — opt-in per SuccessFill's deniability contract. */}
      <SuccessFill active={justInstalledIds.has(app.id)} label={`${app.name} installed`} />
      {showCheckbox && (
        <span className="app-checkbox-wrap" onClick={(e) => e.stopPropagation()}>
          <Checkbox
            checked={selectedApps.has(app.id)}
            onChange={() => toggleApp(app.id)}
            className="app-checkbox"
            disabled={!canSelect}
            ariaLabel={`Select ${app.name}`}
          />
        </span>
      )}
      <AppIcon id={app.id} category={app.category} iconData={app.iconData} />
      <div className="app-info">
        <span className="app-name">
          {app.name}
          {installedApps.has(app.id) && !updateAvailableApps.has(app.id) && (
            <Icon icon="tick" size={12} className="installed-badge" title="Installed" />
          )}
        </span>
        <span className="app-description">{app.description}</span>
        {updateAvailableApps.has(app.id) && app.version && app.availableVersion && (
          <span className="app-version mono">{app.version} → {app.availableVersion}</span>
        )}
      </div>
      {updateAvailableApps.has(app.id) && (
        <Button
          icon={upgradingApp === app.id ? undefined : "refresh"}
          small
          minimal
          intent="warning"
          className="app-update-btn app-card-action--update"
          onClick={(e) => handleUpgradeSingle(app.id, e)}
          disabled={upgradingApp !== null || installing}
          aria-label={`Update ${app.name}`}
          title={`Update ${app.name}`}
        >
          {upgradingApp === app.id && <Spinner size={12} />}
        </Button>
      )}
      {/* Inline single-app install button stays visible alongside the
          checkbox so users can either multi-select or one-click install.
          Loading/disabled is per-id so other cards don't show spinners. */}
      {!installedApps.has(app.id) && !updateAvailableApps.has(app.id) && (() => {
        const isThisInstalling = installingIds.has(app.id);
        return (
          <Button
            icon={isThisInstalling ? undefined : "download"}
            small
            minimal
            intent="success"
            className="app-update-btn app-card-action--download"
            onClick={(e) => { e.stopPropagation(); void installApps([app.id]); }}
            disabled={isThisInstalling || wingetStatus === "failed"}
            loading={isThisInstalling}
            aria-label={`Install ${app.name}`}
            title={`Install ${app.name}`}
          />
        );
      })()}
    </div>
    );
  };

  const renderUpgradeCard = (item: UpgradeItem) => (
    <div
      key={item.id}
      className={`app-card app-card--upgrade update-available ${selectedApps.has(item.id) ? "selected" : ""}`}
      onClick={() => toggleApp(item.id)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          toggleApp(item.id);
        }
      }}
    >
      <span className="app-checkbox-wrap" onClick={(e) => e.stopPropagation()}>
        <Checkbox
          checked={selectedApps.has(item.id)}
          onChange={() => toggleApp(item.id)}
          className="app-checkbox"
          ariaLabel={`Select ${item.name || item.id}`}
        />
      </span>
      <AppIcon id={item.id} category="misc" iconData={item.iconData} />
      <div className="app-info">
        <span className="app-name app-name--truncate" title={item.name || item.id}>{item.name || item.id}</span>
        <span className="app-description">Detected by Windows package inventory</span>
        <span className="app-version mono">{item.version || "?"} → {item.availableVersion || "?"}</span>
      </div>
      <Button
        icon={upgradingApp === item.id ? undefined : "refresh"}
        small
        minimal
        intent="warning"
        className="app-update-btn app-card-action--update"
        onClick={(e) => handleUpgradeSingle(item.id, e)}
        disabled={upgradingApp !== null || installing}
        aria-label={`Update ${item.name || item.id}`}
        title={`Update ${item.name || item.id}`}
      >
        {upgradingApp === item.id && <Spinner size={12} />}
      </Button>
    </div>
  );

  return (
    <div className="app-installer-panel">
      <Card className="installer-card">
        <div className="installer-toolbar-context">
          <span className={`winget-status ${wingetStatus === "installed" ? "installed" : wingetStatus === "not-installed" ? "not-installed" : ""}`}>
            {wingetStatus === "checking" && <Spinner size={14} />}
            {wingetStatus === "installed" && <><Icon icon="tick-circle" size={14} /> PACKAGE MANAGER INSTALLED</>}
            {wingetStatus === "not-installed" && <><Icon icon="cross-circle" size={14} /> PACKAGE MANAGER MISSING</>}
            {wingetStatus === "installing" && <><Spinner size={14} /> INSTALLING...</>}
            {wingetStatus === "failed" && <><Icon icon="error" size={14} /> PACKAGE MANAGER FAILED</>}
          </span>
          <span className={`vulnerability-badge ${vulnerabilityBadge.tone}`}>
            <Icon icon={vulnerabilityBadge.tone === "is-safe" ? "shield" : vulnerabilityBadge.tone === "is-risk" ? "warning-sign" : vulnerabilityBadge.tone === "is-warning" ? "issue" : "info-sign"} size={12} />
            <span>{vulnerabilityBadge.text}</span>
          </span>
        </div>
        <div className="installer-toolbar-layout">
          <div className="installer-toolbar-search">
            <span className="installer-control-label">Search</span>
            <div className="installer-search-box">
              <Icon icon="search" className="installer-search-icon" />
              <InputGroup
                placeholder="Search software catalog by name..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="search-input w-full installer-search-input"
                disabled={appsLoading}
              />
            </div>
          </div>

          <div className="installer-filter-row">
            <span className="installer-control-label">Category</span>
            <div className="category-chips" role="group" aria-label="App categories">
              {CATEGORY_TABS.map((tab) => (
                <button
                  key={tab.id}
                  type="button"
                  className={`category-chip ${selectedCategory === tab.id ? "on" : ""}`}
                  onClick={() => setSelectedCategory(tab.id)}
                  disabled={appsLoading}
                >
                  {tab.label}
                </button>
              ))}
            </div>
            {selectedCategory !== "all" && (
              <Button
                icon="cross"
                minimal
                small
                text="CLEAR FILTER"
                className="category-clear-btn"
                onClick={() => setSelectedCategory("all")}
                disabled={appsLoading}
              />
            )}
          </div>

          <div className="installer-actions-row">
              <div className="installer-selection-count">
                <span className={`mono text-xl leading-none transition-all duration-300 font-bold ${selectedApps.size > 0 ? "text-[var(--color-accent)]" : "text-[var(--color-text-muted)] opacity-30"}`}>
                  {selectedApps.size}
                </span>
                <span className={`mono text-[10px] uppercase tracking-wider font-semibold ${selectedApps.size > 0 ? "text-[var(--color-accent)]" : "text-[var(--color-text-muted)] opacity-40"}`}>
                  Selected
                </span>
              </div>

              <div className="installer-action-buttons">
              <div className="installer-action-buttons-utility">
              <Button
                icon={contextLoading?.apps ? undefined : "refresh"}
                text="REFRESH"
                minimal
                className="installer-toolbar-btn installer-toolbar-btn--accent font-mono text-[11px]!"
                onClick={handleRefreshAll}
                disabled={contextLoading?.apps || appsLoading || inventoryScanPending}
                loading={contextLoading?.apps || inventoryScanPending}
              />
              <div className="h-4 w-px bg-[var(--color-border)] mx-2" />
              <Button
                icon="plus"
                text="SELECT ALL"
                minimal
                className="installer-toolbar-btn installer-toolbar-btn--accent font-mono text-[11px]!"
                onClick={selectAllInCategory}
                disabled={appsLoading}
              />
              <Button
                icon="trash"
                text="CLEAR"
                minimal
                className="installer-toolbar-btn installer-toolbar-btn--danger font-mono text-[11px]!"
                onClick={clearSelection}
                disabled={appsLoading || selectedApps.size === 0}
              />
              </div>
              <div className="installer-action-buttons-bulk">
              {/* Tour anchor: the pending-update COUNT itself is shown by the
                  vulnerability badge in the status row above the card (e.g.
                  "N APPS NEED UPDATES"), not inside this button group — so this
                  wrap stays tight around just the actionable Update controls
                  rather than reaching up to grab an unrelated sibling. */}
              <div className="installer-update-controls inline-flex items-center gap-2" data-tour="apps-update-section">
              {/* Always rendered (not gated on outdatedInstalledCount) so this
                  wrapper never collapses to zero size — the apps-tour-updates
                  tour step falls back to this anchor when there's nothing
                  pending in apps-updates-grid, and an empty wrapper measures
                  to nothing for the tour to highlight. Disabled instead of
                  hidden when there's nothing to update. */}
              {outdatedInstalledCount > 0 ? (
                <Button
                  icon="automatic-updates"
                  text="UPDATE ALL"
                  minimal
                  className="app-update-all-btn font-mono text-[11px]!"
                  onClick={handleUpdateAll}
                  disabled={localLoadingMap["updateAll"] || appsLoading}
                  loading={localLoadingMap["updateAll"]}
                />
              ) : (
                <Button
                  icon="tick-circle"
                  text="UP TO DATE"
                  minimal
                  disabled
                  className="app-update-all-btn font-mono text-[11px]!"
                />
              )}
              {selectedUpdateIds.length > 0 && (
                <Button
                  icon={localLoadingMap["updateSelected"] ? undefined : "automatic-updates"}
                  text={`UPDATE SELECTED (${selectedUpdateIds.length})`}
                  minimal
                  small
                  className="app-update-selected-btn font-mono text-[10px]! tracking-wide"
                  onClick={handleUpdateSelected}
                  loading={localLoadingMap["updateSelected"]}
                  disabled={localLoadingMap["updateSelected"] || localLoadingMap["updateAll"] || appsLoading}
                />
              )}
              </div>
              {edgeInstalled && (
                <Button
                icon="trash"
                text="REMOVE EDGE"
                minimal
                className="installer-toolbar-btn installer-toolbar-btn--danger font-mono text-[11px]!"
                onClick={() => runRemoval("removeEdge", removeEdge)}
                disabled={localLoadingMap["removeEdge"] || appsLoading}
                loading={localLoadingMap["removeEdge"]}
                />
              )}
              {oneDriveInstalled && (
                <Button
                icon="cloud-download"
                text="REMOVE ONEDRIVE"
                minimal
                className="installer-toolbar-btn installer-toolbar-btn--danger font-mono text-[11px]!"
                onClick={() => runRemoval("removeOneDrive", removeOneDrive)}
                disabled={localLoadingMap["removeOneDrive"] || appsLoading}
                loading={localLoadingMap["removeOneDrive"]}
                />
              )}
              {teamsInstalled && (
                <Button
                icon="people"
                text="REMOVE TEAMS"
                minimal
                className="installer-toolbar-btn installer-toolbar-btn--danger font-mono text-[11px]!"
                onClick={() => runRemoval("removeTeams", removeTeams)}
                disabled={localLoadingMap["removeTeams"] || appsLoading}
                loading={localLoadingMap["removeTeams"]}
                />
              )}
              {/* Stays enabled during a run: the label switches to ADD TO
                  INSTALL because that is what a click now does — the apps join
                  the in-flight install instead of being refused or needing a
                  separate SCHEDULE press. Deliberately NOT `loading`, which
                  would disable it and take that away; the per-card spinners and
                  the Processes row already show the run. */}
              {selectedInstallIds.length > 0 && (
                <Button
                  icon="download"
                  text={`${installing ? "ADD TO INSTALL" : "INSTALL APPS"} (${selectedInstallIds.length})`}
                  intent="success"
                  minimal
                  small
                  className="app-install-btn font-mono text-[10px]! tracking-wide"
                  onClick={handleInstall}
                  disabled={wingetStatus === "failed" || appsLoading}
                />
              )}
              </div>
              </div>
          </div>
        </div>
        {/* Inner sub-tabs for the catalog grid, mirroring Browser Hardening's
            per-item Tabs pattern (Privacy). "Updates" combines what were
            previously two stacked groups — the collapsible "Updates
            Available" and the always-shown "Other Updates" — into one tab. */}
        <div ref={appsGridRef}>
          <Tabs value={installerView} onValueChange={(value) => setInstallerView(value as typeof installerView)}>
            <TabsList className="w-full flex-wrap justify-start">
              <TabsTrigger value="not-installed">Not Installed ({notInstalledApps.length})</TabsTrigger>
              <TabsTrigger value="updates">Updates ({outdatedInstalledCount})</TabsTrigger>
              <TabsTrigger value="installed">Installed ({noActionApps.length})</TabsTrigger>
            </TabsList>

            {/* Tour anchor: wraps only the curated/installable catalog so the
                highlight targets this tab's grid specifically. */}
            <TabsContent value="not-installed">
              <div className="apps-grid" data-tour="apps-utility-section">
                {appsLoading ? (
                  <div className="empty-state">
                    <Icon icon="time" size={32} className="scanning-icon" />
                    <p>Loading app catalog...</p>
                  </div>
                ) : notInstalledApps.length === 0 ? (
                  <div className="empty-state">
                    <Icon icon="search" size={32} className="empty-icon" />
                    <p>No apps found in this category</p>
                  </div>
                ) : (
                  notInstalledApps.map(app => renderApp(app, true))
                )}
              </div>
            </TabsContent>

            {/* Tour anchor: wraps the actual per-app update cards (both
                manifest apps with updateAvailable and "other" packages from
                the winget inventory scan). The tour's anchor resolver falls
                back to the apps-update-section button anchor when this tab
                isn't the active inner tab, so gating stays correct. */}
            <TabsContent value="updates">
              {updatesTools && <div className="mb-6">{updatesTools}</div>}
              <div className="apps-grid" data-tour="apps-updates-grid">
                {appsLoading ? (
                  <div className="empty-state">
                    <Icon icon="time" size={32} className="scanning-icon" />
                    <p>Loading app catalog...</p>
                  </div>
                ) : outdatedInstalledCount === 0 ? (
                  <div className="empty-state">
                    <Icon icon="tick-circle" size={32} className="empty-icon" />
                    <p>Nothing needs an update</p>
                  </div>
                ) : (
                  <>
                    {updateApps.length > 0 && (
                      <>
                        <div className="grid-divider">
                          <div className="divider-line"></div>
                          <div className="divider-label">FROM THE CATALOG ({updateApps.length})</div>
                          <div className="divider-line"></div>
                        </div>
                        <div className="app-group-grid app-group-grid--updates">
                          {updateApps.map((app, idx) => (
                            // Staggered fade+rise on entrance so items arrive
                            // in sequence rather than all at once.
                            <motion.div
                              key={app.id}
                              initial={{ opacity: 0, y: 6 }}
                              animate={{ opacity: 1, y: 0 }}
                              transition={{
                                delay: staggerDelay(idx),
                                duration: DURATION_S.normal,
                                ease: EASE.enter,
                              }}
                            >
                              {renderApp(app)}
                            </motion.div>
                          ))}
                        </div>
                      </>
                    )}

                    {/* Other system-wide updates — packages winget reports
                        outside the curated manifest. */}
                    {filteredOtherUpgrades.length > 0 && (
                      <>
                        <div className="grid-divider">
                          <div className="divider-line"></div>
                          <div className="divider-label">OTHER PACKAGES ({filteredOtherUpgrades.length})</div>
                          <div className="divider-line"></div>
                        </div>
                        <div className="app-group-grid app-group-grid--updates">
                          {filteredOtherUpgrades.map(renderUpgradeCard)}
                        </div>
                      </>
                    )}
                  </>
                )}
              </div>
            </TabsContent>

            <TabsContent value="installed">
              <div className="apps-grid">
                {appsLoading ? (
                  <div className="empty-state">
                    <Icon icon="time" size={32} className="scanning-icon" />
                    <p>Loading app catalog...</p>
                  </div>
                ) : noActionApps.length === 0 ? (
                  <div className="empty-state">
                    <Icon icon="search" size={32} className="empty-icon" />
                    <p>No apps installed in this category</p>
                  </div>
                ) : (
                  noActionApps.map((app, idx) => (
                    // Staggered fade+rise: items enter in sequence, capped
                    // by staggerDelay so long lists never drag.
                    <motion.div
                      key={app.id}
                      initial={{ opacity: 0, y: 6 }}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{
                        delay: staggerDelay(idx),
                        duration: DURATION_S.normal,
                        ease: EASE.enter,
                      }}
                    >
                      {renderApp(app)}
                    </motion.div>
                  ))
                )}
              </div>
            </TabsContent>
          </Tabs>
        </div>

      </Card >
    </div >
  );
}

export default AppInstallerPanel;
