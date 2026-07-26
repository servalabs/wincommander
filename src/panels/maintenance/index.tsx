import { useEffect, useRef } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import PanelHeader from "../../components/shared/PanelHeader";
import TierGate from "../../components/shared/TierGate";
import { FileHygieneTools } from "./FileHygieneTools";
import { RegistryTools } from "./RegistryTools";
import { MalwareCenter } from "./MalwareCenter";
import { SecurityData } from "./SecurityData";
import { SystemHygieneTools } from "./SystemHygieneTools";
import { PerformanceTools } from "./PerformanceTools";
import { StartupDriverTools } from "./StartupDriverTools";
import OsRepairCard from "./OsRepairCard";
import ReclaimSpaceCard from "./ReclaimSpaceCard";
import DiskSpaceAnalyzerDialog from "./DiskSpaceAnalyzerDialog";
import "./DiskSpaceAnalyzerDialog.css";
import FileStatsPanel from "./FileStatsPanel";
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
      <PanelHeader
        panelId="maintenance"
        title="System Maintenance"
        description="Reclaim space, repair Windows, and check how it starts and performs. Privacy and trace erasure live in System Cleanup; software updates live in Packages & Apps."
      />

      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <TabsList className="w-full flex-wrap justify-start">
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="files">Storage &amp; files</TabsTrigger>
          <TabsTrigger value="system">Repair &amp; hygiene</TabsTrigger>
          <TabsTrigger value="registry">Registry &amp; Explorer</TabsTrigger>
          <TabsTrigger value="performance">Performance</TabsTrigger>
          <TabsTrigger value="startup">Startup &amp; drivers</TabsTrigger>
          <TabsTrigger value="security">Security Center</TabsTrigger>
        </TabsList>
        <TabsContent value="overview"><MaintenanceOverview onOpenSection={setActiveTab} /></TabsContent>
        <TabsContent value="files"><StorageAndFileTools cleanupRef={diskCleanupRef} analyzerRef={diskAnalyzerRef} /></TabsContent>
        <TabsContent value="registry"><RegistryTools /></TabsContent>
        <TabsContent value="system"><RepairAndHygieneTools /></TabsContent>
        <TabsContent value="performance"><PerformanceTools /></TabsContent>
        <TabsContent value="startup"><StartupDriverTools /></TabsContent>
        <TabsContent value="security"><SecurityCenter /></TabsContent>
      </Tabs>
    </div>
  );
}
type MaintenanceSection = "files" | "system" | "registry" | "performance" | "startup" | "security";
type MaintenanceIcon = "search" | "wrench" | "cog" | "pulse" | "time" | "shield";

function MaintenanceOverview({ onOpenSection }: { onOpenSection: (section: MaintenanceSection) => void }) {
  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <MaintenanceGroup title="Clean, reclaim, and repair" description="Recover space and correct local clutter or configuration problems." tools={[
        ["Storage & files", "Reclaim Windows and application storage, then inspect what is actually using the disk.", "files", "search"],
        ["Repair & hygiene", "Run SFC/DISM, Windows Update repair, and defrag; audit broken shortcuts, stale PATH entries, and uninstall leftovers.", "system", "wrench"],
        ["Registry & Explorer", "Review orphaned registrations and third-party context-menu entries.", "registry", "cog"],
      ]} onOpenSection={onOpenSection} />
      <MaintenanceGroup title="Run and protect Windows" description="Inspect how Windows starts and performs, and check its local security posture." tools={[
        ["Performance", "Check CPU, memory, disks, adapters, and top processes.", "performance", "pulse"],
        ["Startup & drivers", "Review startup impact and signed Plug-and-Play driver inventory.", "startup", "time"],
        ["Security Center", "Microsoft Defender response and local security posture.", "security", "shield"],
      ]} onOpenSection={onOpenSection} />
    </div>
  );
}
function StorageAndFileTools({ cleanupRef, analyzerRef }: { cleanupRef: React.RefObject<HTMLDivElement | null>; analyzerRef: React.RefObject<HTMLDivElement | null> }) {
  return (
    <div className="flex flex-col gap-4">
      <div ref={cleanupRef}><ReclaimSpaceCard /></div>
      <div className="grid gap-4 xl:grid-cols-2"><FileHygieneTools /><FileStatsPanel /></div>
      <Card>
        <CardHeader><CardTitle>Disk Space Analyzer</CardTitle><CardDescription>Inspect disk usage and large items before taking action. This remains an analysis tool and does not delete files automatically.</CardDescription></CardHeader>
        <CardContent ref={analyzerRef} className="min-w-0"><div className="max-h-[70vh] overflow-auto"><DiskSpaceAnalyzerDialog inline isOpen={true} onClose={() => {}} initialMode="space" /></div></CardContent>
      </Card>
    </div>
  );
}

function RepairAndHygieneTools() {
  return (
    <div className="flex flex-col gap-4">
      <OsRepairCard />
      <SystemHygieneTools />
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

function MaintenanceGroup({ title, description, tools, onOpenSection }: { title: string; description: string; tools: [string, string, MaintenanceSection, MaintenanceIcon][]; onOpenSection: (section: MaintenanceSection) => void }) {
  return <Card><CardHeader><CardTitle>{title}</CardTitle><CardDescription>{description}</CardDescription></CardHeader><CardContent className="flex flex-col gap-2">{tools.map(([label, detail, section, icon]) => <button key={section} type="button" onClick={() => onOpenSection(section)} className="flex items-start gap-3 rounded-[var(--r)] border border-[var(--border)] p-3 text-left transition-colors hover:bg-[var(--surface-2)]"><Icon icon={icon} className="mt-0.5 text-[var(--accent)]" /><span className="min-w-0"><span className="block text-sm font-medium text-[var(--text)]">{label}</span><span className="block text-xs text-[var(--text-dim)]">{detail}</span></span></button>)}</CardContent></Card>;
}
