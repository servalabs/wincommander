import { useEffect, useRef } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import TierGate from "../../components/shared/TierGate";
import { FileHygieneTools } from "./FileHygieneTools";
import { RegistryTools } from "./RegistryTools";
import { MalwareCenter } from "./MalwareCenter";
import { SecurityData } from "./SecurityData";
import { SystemHygieneTools } from "./SystemHygieneTools";
import { PerformanceTools } from "./PerformanceTools";
import { StartupDriverTools } from "./StartupDriverTools";
import { RoutineCleanerPanel } from "./RoutineCleanerPanel";
import { WINDOWS_DISK_CLEANUP_CATEGORIES } from "./routineCleanerHelpers";
import DiskCleanupGranular from "../../components/tweaks/managers/DiskCleanupGranular";
import DiskSpaceAnalyzerDialog from "../tweaks/DiskSpaceAnalyzerDialog";
import "../tweaks/DiskSpaceAnalyzerDialog.css";
import FileStatsPanel from "../tweaks/FileStatsPanel";
import "./MaintenanceStorage.css";
import { useMaintenanceSessionState } from "./maintenanceSessionState";

export default function MaintenancePanel() {
  const [activeTab, setActiveTab] = useMaintenanceSessionState("maintenance.active-tab", "overview");
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

  return (
    <div className="panel-container flex flex-col gap-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className="font-mono text-[11px] uppercase tracking-[0.14em] text-[var(--accent)]">Maintenance</p>
          <h1 className="mt-1 font-[family-name:var(--font-display)] text-2xl font-semibold text-[var(--text)]">System Maintenance</h1>
          <p className="mt-1 max-w-2xl text-sm text-[var(--text-dim)]">Windows care in one place: cleanup, repair, storage, performance, startup, drivers, and local security posture. Network-policy and privacy tools remain in their specialist homes.</p>
        </div>
        <Badge tone="accent">Disk cleanup</Badge>
      </div>

      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <TabsList className="w-full flex-wrap justify-start">
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="files">Storage &amp; files</TabsTrigger>
          <TabsTrigger value="system">System repair</TabsTrigger>
          <TabsTrigger value="registry">Registry &amp; Explorer</TabsTrigger>
          <TabsTrigger value="performance">Performance</TabsTrigger>
          <TabsTrigger value="startup">Startup &amp; drivers</TabsTrigger>
          <TabsTrigger value="security">Security Center</TabsTrigger>
        </TabsList>
        <TabsContent value="overview"><MaintenanceOverview onOpenSection={setActiveTab} /></TabsContent>
        <TabsContent value="files"><StorageAndFileTools cleanupRef={diskCleanupRef} analyzerRef={diskAnalyzerRef} /></TabsContent>
        <TabsContent value="registry"><RegistryTools /></TabsContent>
        <TabsContent value="system"><SystemHygieneTools /></TabsContent>
        <TabsContent value="performance"><PerformanceTools /></TabsContent>
        <TabsContent value="startup"><StartupDriverTools /></TabsContent>
        <TabsContent value="security"><SecurityCenter /></TabsContent>
      </Tabs>
    </div>
  );
}
type MaintenanceSection = "files" | "system" | "registry" | "performance" | "startup" | "security";

function MaintenanceOverview({ onOpenSection }: { onOpenSection: (section: MaintenanceSection) => void }) {
  const openPanel = (panel: "apps" | "network" | "vault" | "tweaks") => {
    window.dispatchEvent(new CustomEvent("navigate-panel", { detail: panel }));
  };

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Windows system care</CardTitle>
          <CardDescription>Use these tools for the device itself. Every existing maintenance capability remains available; related tools are grouped by the job they perform.</CardDescription>
        </CardHeader>
      </Card>
      <div className="grid gap-4 xl:grid-cols-2">
        <MaintenanceGroup title="Clean, reclaim, and repair" description="Recover space and correct local clutter or configuration issues." tools={[
          ["Storage & files", "Run Windows and application cache cleanup, then inspect disk usage before removal.", "files", "search"],
          ["System repair", "Review broken shortcuts, stale PATH entries, and uninstall leftovers.", "system", "wrench"],
          ["Registry & Explorer", "Review orphaned registrations and third-party context-menu entries.", "registry", "cog"],
        ]} onOpenSection={onOpenSection} />
        <MaintenanceGroup title="Run and connect Windows" description="Inspect how Windows starts, performs, and communicates through its adapters." tools={[
          ["Performance", "Check CPU, memory, disks, adapters, and top processes.", "performance", "pulse"],
          ["Startup & drivers", "Review startup impact and signed Plug-and-Play driver inventory.", "startup", "time"],
        ]} onOpenSection={onOpenSection} />
      </div>
      <Card>
        <CardHeader>
          <CardTitle>Related specialist panels</CardTitle>
          <CardDescription>Packages &amp; Apps owns software updates. Network Control, Secure Storage, and Windows Settings retain their specialist workflows.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={() => onOpenSection("security")}><Icon icon="shield" />Security Center</Button>
          <Button variant="ghost" onClick={() => openPanel("apps")}><Icon icon="applications" />Packages &amp; Apps</Button>
          <Button variant="ghost" onClick={() => openPanel("network")}><Icon icon="globe-network" />Network Control</Button>
          <Button variant="ghost" onClick={() => openPanel("vault")}><Icon icon="lock" />Secure Storage</Button>
          <Button variant="ghost" onClick={() => openPanel("tweaks")}><Icon icon="cog" />Windows Settings</Button>
        </CardContent>
      </Card>
    </div>
  );
}
function StorageAndFileTools({ cleanupRef, analyzerRef }: { cleanupRef: React.RefObject<HTMLDivElement | null>; analyzerRef: React.RefObject<HTMLDivElement | null> }) {
  return (
    <div className="flex flex-col gap-4">
      <div className="maintenance-cleanup-grid">
        <Card className="maintenance-cleanup-card flex flex-col">
          <CardHeader>
            <CardTitle>Windows disk cleanup</CardTitle>
            <CardDescription>Preview and reclaim Windows-managed temporary files, update cache, logs, and other system-owned storage. Existing schedules and backend safeguards are unchanged.</CardDescription>
          </CardHeader>
          <CardContent className="min-w-0 flex-1"><div ref={cleanupRef} data-tour="maintenance-disk-cleanup"><DiskCleanupGranular /></div></CardContent>
        </Card>
        <RoutineCleanerPanel
          categories={WINDOWS_DISK_CLEANUP_CATEGORIES}
          title="Application cache cleanup"
          description="Preview and reclaim regenerable browser, application, gaming, and database cache data alongside Windows cleanup. Cleaning remains selective and requires confirmation."
        />
      </div>
      <div className="grid gap-4 xl:grid-cols-2"><FileHygieneTools /><FileStatsPanel /></div>
      <Card>
        <CardHeader><CardTitle>Disk Space Analyzer</CardTitle><CardDescription>Inspect disk usage and large items before taking action. This remains an analysis tool and does not delete files automatically.</CardDescription></CardHeader>
        <CardContent ref={analyzerRef} className="min-w-0"><div className="max-h-[70vh] overflow-auto"><DiskSpaceAnalyzerDialog inline isOpen={true} onClose={() => {}} initialMode="space" /></div></CardContent>
      </Card>
    </div>
  );
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

function MaintenanceGroup({ title, description, tools, onOpenSection }: { title: string; description: string; tools: [string, string, MaintenanceSection, "clean" | "search" | "wrench" | "cog" | "pulse" | "time" | "globe-network"][]; onOpenSection: (section: MaintenanceSection) => void }) {
  return <Card><CardHeader><CardTitle>{title}</CardTitle><CardDescription>{description}</CardDescription></CardHeader><CardContent className="flex flex-col gap-2">{tools.map(([label, detail, section, icon]) => <button key={section} type="button" onClick={() => onOpenSection(section)} className="flex items-start gap-3 rounded-[var(--r)] border border-[var(--border)] p-3 text-left transition-colors hover:bg-[var(--surface-2)]"><Icon icon={icon} className="mt-0.5 text-[var(--accent)]" /><span className="min-w-0"><span className="block text-sm font-medium text-[var(--text)]">{label}</span><span className="block text-xs text-[var(--text-dim)]">{detail}</span></span></button>)}</CardContent></Card>;
}
