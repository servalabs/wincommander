import { useEffect, useRef } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import PanelHeader from "../../components/shared/PanelHeader";
import TierGate from "../../components/shared/TierGate";
import { useBackend } from "../../hooks/useBackend";
import { RegistryTools } from "./RegistryTools";
import { MalwareCenter } from "./MalwareCenter";
import { SecurityData } from "./SecurityData";
import { SystemHygieneTools } from "./SystemHygieneTools";
import { StartupDriverTools } from "./StartupDriverTools";
import { AppBrowserCacheCard, WindowsStorageCard } from "./ReclaimSpaceCard";
import DiskSpaceAnalyzerDialog from "./DiskSpaceAnalyzerDialog";
import "./DiskSpaceAnalyzerDialog.css";
import FileStatsPanel from "./FileStatsPanel";
import "./MaintenanceStorage.css";
import { getMaintenanceSessionValue, primeMaintenanceSessionValue, useMaintenanceSessionState } from "./maintenanceSessionState";
import { APP_CACHE_CLEANUP_CATEGORIES, getRecommendedItemIds } from "./routineCleanerHelpers";

const APP_CACHE_SESSION_KEY = `routine-cleaner.${[...APP_CACHE_CLEANUP_CATEGORIES].sort().join("-")}`;

export default function MaintenancePanel() {
  const [activeTab, setActiveTab] = useMaintenanceSessionState("maintenance.active-tab", "files");
  const diskCleanupRef = useRef<HTMLDivElement>(null);
  const diskAnalyzerRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const openStorage = (target: "cleanup" | "analyzer") => {
      setActiveTab("files");
      window.setTimeout(() => (target === "cleanup" ? diskCleanupRef : diskAnalyzerRef).current?.scrollIntoView({ behavior: "smooth", block: "start" }), 80);
    };
    const openCleanup = () => openStorage("cleanup");
    const openAnalyzer = () => openStorage("analyzer");
    const openStorageTab = () => setActiveTab("files");
    window.addEventListener("open-disk-cleanup", openCleanup);
    window.addEventListener("open-disk-space-analyzer", openAnalyzer);
    window.addEventListener("open-maintenance-storage", openStorageTab);
    return () => {
      window.removeEventListener("open-disk-cleanup", openCleanup);
      window.removeEventListener("open-disk-space-analyzer", openAnalyzer);
      window.removeEventListener("open-maintenance-storage", openStorageTab);
    };
  }, [setActiveTab]);

  useMaintenanceReviewPreload();

  return (
    <div className="panel-container flex flex-col gap-4">
      <PanelHeader
        panelId="maintenance"
        title="System Maintenance"
        description="Reclaim space and check startup, drivers, and security. Windows repair, privacy, and trace erasure live in System Cleanup; software updates live in Packages & Apps."
      />

      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <TabsList className="w-full flex-wrap justify-start">
          <TabsTrigger value="files">Storage &amp; files</TabsTrigger>
          <TabsTrigger value="registry">Registry &amp; cleanup</TabsTrigger>
          <TabsTrigger value="startup">Startup &amp; drivers</TabsTrigger>
          <TabsTrigger value="security">Security Center</TabsTrigger>
        </TabsList>
        <TabsContent value="files"><StorageAndFileTools cleanupRef={diskCleanupRef} analyzerRef={diskAnalyzerRef} /></TabsContent>
        <TabsContent value="registry"><RegistryAndHygieneTools /></TabsContent>
        <TabsContent value="startup"><StartupDriverTools /></TabsContent>
        <TabsContent value="security"><SecurityCenter /></TabsContent>
      </Tabs>
    </div>
  );
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
function StorageAndFileTools({ cleanupRef, analyzerRef }: { cleanupRef: React.RefObject<HTMLDivElement | null>; analyzerRef: React.RefObject<HTMLDivElement | null> }) {
  return (
    <div className="flex flex-col gap-4">
      <div className="maintenance-storage-workspace">
        <div ref={cleanupRef} className="min-w-0"><WindowsStorageCard /></div>
        <AppBrowserCacheCard />
      </div>
      <div className="maintenance-analysis-workspace">
      <Card className="maintenance-analysis-card">
        <CardHeader><CardTitle>Detailed folder map</CardTitle><CardDescription>Inspect disk usage and large items before taking action. This remains an analysis tool and does not delete files automatically.</CardDescription></CardHeader>
        <CardContent ref={analyzerRef} className="maintenance-analysis-content"><div className="maintenance-analysis-scroll"><DiskSpaceAnalyzerDialog inline isOpen={true} onClose={() => {}} initialMode="space" /></div></CardContent>
      </Card>
      <div className="maintenance-file-stats-slot"><FileStatsPanel /></div>
      </div>
    </div>
  );
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

function SecurityCenter() {
  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Security Center</CardTitle>
          <CardDescription>Microsoft Defender response and local security posture are kept together. Cleanup tools do not remove protection history from this workflow.</CardDescription>
        </CardHeader>
      </Card>
      <TierGate tier="paid" featureLabel="Malware scanning"><MalwareCenter /></TierGate>
      <TierGate tier="paid" featureLabel="Security data"><SecurityData /></TierGate>
    </div>
  );
}
