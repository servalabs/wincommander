import { useEffect, useRef } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
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

  return (
    <div className="panel-container flex flex-col gap-4">
      <PanelHeader
        panelId="maintenance"
        title="System Maintenance"
        description="Reclaim space, repair Windows, and check how it starts and performs. Privacy and trace erasure live in System Cleanup; software updates live in Packages & Apps."
      />

      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <TabsList className="w-full flex-wrap justify-start">
          <TabsTrigger value="files">Storage &amp; files</TabsTrigger>
          <TabsTrigger value="system">Repair &amp; hygiene</TabsTrigger>
          <TabsTrigger value="registry">Registry &amp; Explorer</TabsTrigger>
          <TabsTrigger value="performance">Performance</TabsTrigger>
          <TabsTrigger value="startup">Startup &amp; drivers</TabsTrigger>
          <TabsTrigger value="security">Security Center</TabsTrigger>
        </TabsList>
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
      <RepairSectionLabel>OS repair</RepairSectionLabel>
      <OsRepairCard />
      <div className="h-px bg-[var(--border)]" />
      <RepairSectionLabel>Cleanup &amp; hygiene</RepairSectionLabel>
      <SystemHygieneTools />
    </div>
  );
}

// Distinguishes SFC/DISM/defrag (OsRepairCard) from shortcuts/environment/
// uninstall-leftover audits (SystemHygieneTools) -- both sat under one
// ambiguous "Repair & hygiene" tab and read as duplicates of each other.
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
