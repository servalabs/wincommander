import { useEffect, useRef } from "react";
import { Card, CardContent } from "../../components/ui/card";
import PanelHeader from "../../components/shared/PanelHeader";
import { useBackend } from "../../hooks/useBackend";
import { RegistryTools } from "./RegistryTools";
import { SystemHygieneTools } from "./SystemHygieneTools";
import { StartupDriverTools } from "./StartupDriverTools";
import { AppBrowserCacheCard, RoutineCacheCleanupCard, WindowsStorageCard } from "./ReclaimSpaceCard";
import { FileHygieneTools } from "./FileHygieneTools";
import { MAINTENANCE_WORKSPACES, MaintenanceWorkspaceNav } from "./workspace/MaintenanceWorkspaceNav";
import MaintenanceHistoryPanel from "./history/MaintenanceHistoryPanel";
import DiskSpaceAnalyzerDialog from "./DiskSpaceAnalyzerDialog";
import "./DiskSpaceAnalyzerDialog.css";
import "./MaintenanceStorage.css";
import { getMaintenanceSessionValue, primeMaintenanceSessionValue, useMaintenanceSessionState } from "./maintenanceSessionState";
import { APP_CACHE_CLEANUP_CATEGORIES, getRecommendedItemIds } from "./routineCleanerHelpers";

const APP_CACHE_SESSION_KEY = `routine-cleaner.${[...APP_CACHE_CLEANUP_CATEGORIES].sort().join("-")}`;

export default function MaintenancePanel() {
  const [windowsOpen, setWindowsOpen] = useMaintenanceSessionState("maintenance.windows-storage-open", false);
  const [activeTab, setActiveTab] = useMaintenanceSessionState("maintenance.active-workspace", "cleanup");
  const active = MAINTENANCE_WORKSPACES.find((item) => item.id === activeTab) ?? MAINTENANCE_WORKSPACES[0];
  useEffect(() => {
    const openCleanup = () => { setActiveTab("cleanup"); setWindowsOpen(true); };
    const openStorage = () => setActiveTab("cleanup");
    const openAnalyzer = () => setActiveTab("analysis");
    window.addEventListener("open-disk-cleanup", openCleanup);
    window.addEventListener("open-disk-space-analyzer", openAnalyzer);
    window.addEventListener("open-maintenance-storage", openStorage);
    return () => {
      window.removeEventListener("open-disk-cleanup", openCleanup);
      window.removeEventListener("open-disk-space-analyzer", openAnalyzer);
      window.removeEventListener("open-maintenance-storage", openStorage);
    };
  }, [setActiveTab, setWindowsOpen]);
  useMaintenanceReviewPreload();
  return <div className="panel-container maintenance-panel flex flex-col gap-4">
    <PanelHeader panelId="maintenance" title="System Maintenance" description="Understand your storage, review cleanable data, and keep your system in order." />
    <div className="maintenance-workspace-shell">
      <MaintenanceWorkspaceNav active={active.id} onChange={setActiveTab} />
      <section className="maintenance-workspace-main" aria-label={active.label}>
        {active.id !== "analysis" && active.id !== "large" && <header className="maintenance-workspace-heading"><h2>{active.label}</h2><p>{active.description}</p></header>}
        {active.id === "cleanup" && <StorageAndFileTools windowsOpen={windowsOpen} setWindowsOpen={setWindowsOpen} />}
        {(active.id === "analysis" || active.id === "large") && <div className="maintenance-analysis-workspace">
          <Card className="maintenance-analysis-card"><CardContent className="maintenance-analysis-content"><div className="maintenance-analysis-scroll"><DiskSpaceAnalyzerDialog key={active.id} inline isOpen onClose={() => {}} initialMode={active.id === "large" ? "large" : "space"} /></div></CardContent></Card>
        </div>}
        {active.id === "duplicates" && <FileHygieneTools />}
        {active.id === "registry" && <RegistryAndHygieneTools />}
        {active.id === "startup" && <StartupDriverTools />}
        {active.id === "history" && <MaintenanceHistoryPanel />}
      </section>
    </div>
  </div>;
}

function useMaintenanceReviewPreload() {
  const backend = useBackend();
  const startedRef = useRef(false);

  useEffect(() => {
    if (startedRef.current || getMaintenanceSessionValue<boolean>("maintenance.review-preload-complete")) return;
    startedRef.current = true;
    primeMaintenanceSessionValue("maintenance.review-preload-complete", true);
    primeMaintenanceSessionValue("registry-hygiene.pre-scanned", true);
    primeMaintenanceSessionValue("registry-hygiene.busy", true);
    primeMaintenanceSessionValue("system-hygiene.pre-scanned", true);
    primeMaintenanceSessionValue("system-hygiene.busy", true);
    primeMaintenanceSessionValue(`${APP_CACHE_SESSION_KEY}.operation`, "scanning");

    void Promise.all([
      backend.registryCleanerScan(),
      backend.explorerContextMenuScan(),
      backend.shortcutCleanerScan(),
      backend.environmentCleanerScan(),
      backend.uninstallLeftoversScan(),
    ]).then(([registry, context, shortcuts, environment, leftovers]) => {
      primeMaintenanceSessionValue("registry-hygiene.registry-scan", registry);
      primeMaintenanceSessionValue("registry-hygiene.context-scan", context);
      primeMaintenanceSessionValue("system-hygiene.shortcuts", shortcuts);
      primeMaintenanceSessionValue("system-hygiene.environment", environment);
      primeMaintenanceSessionValue("system-hygiene.leftovers", leftovers);
    }).catch((cause) => {
      const error = String(cause);
      primeMaintenanceSessionValue("registry-hygiene.error", error);
      primeMaintenanceSessionValue("system-hygiene.error", error);
    }).finally(() => {
      primeMaintenanceSessionValue("registry-hygiene.busy", false);
      primeMaintenanceSessionValue("system-hygiene.busy", false);
    });

    // App/browser caches are a read-only Maintenance-wide preload. The panel
    // consumes this same session entry, so opening Storage & files never starts
    // a second scan and the visible refresh button remains the explicit rescan.
    void backend.routineCleanerScan(APP_CACHE_CLEANUP_CATEGORIES).then((scan) => {
      primeMaintenanceSessionValue(`${APP_CACHE_SESSION_KEY}.scan`, scan);
      primeMaintenanceSessionValue(`${APP_CACHE_SESSION_KEY}.selected`, new Set(getRecommendedItemIds(scan.items)));
    }).catch((cause) => {
      primeMaintenanceSessionValue(`${APP_CACHE_SESSION_KEY}.error`, String(cause));
    }).finally(() => {
      primeMaintenanceSessionValue(`${APP_CACHE_SESSION_KEY}.operation`, "idle");
    });
  }, [backend]);
}
function StorageAndFileTools({ windowsOpen, setWindowsOpen }: { windowsOpen: boolean; setWindowsOpen: (open: boolean) => void }) {
  return <div className="flex flex-col gap-4">
    <AppBrowserCacheCard />
    <details className="maintenance-secondary-scope" open={windowsOpen} onToggle={(event) => setWindowsOpen(event.currentTarget.open)}><summary>Windows storage &amp; routine maintenance</summary>
      <div className="flex flex-col gap-4 pt-4"><WindowsStorageCard /><RoutineCacheCleanupCard /></div>
    </details>
  </div>;
}

function RegistryAndHygieneTools() {
  return (
    <div className="flex flex-col gap-4">
      <RepairSectionLabel>Registry &amp; Explorer</RepairSectionLabel>
      <RegistryTools />
      <div className="h-px bg-[var(--border)]" />
      <RepairSectionLabel>Cleanup &amp; hygiene</RepairSectionLabel>
      <SystemHygieneTools />
    </div>
  );
}

// Distinguishes registry/context-menu audits (RegistryTools) from shortcuts/
// environment/uninstall-leftover audits (SystemHygieneTools) -- both are
// scan-then-select-then-fix workflows grouped into one tab, but they inspect
// different things and read as duplicates of each other without a label.
function RepairSectionLabel({ children }: { children: React.ReactNode }) {
  return <span className="font-[family-name:var(--font-mono)] text-[10.5px] uppercase tracking-wider text-[var(--text-mute)]">{children}</span>;
}
