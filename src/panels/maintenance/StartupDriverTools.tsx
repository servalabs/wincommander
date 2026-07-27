import { useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import StartupManager from "../../components/tweaks/managers/StartupManager";
import LocalUsersManager from "../../components/tweaks/managers/LocalUsersManager";
import ScheduledTasksManager from "../../components/tweaks/managers/ScheduledTasksManager";
import ServiceManager from "../../components/tweaks/managers/ServiceManager";
import { RuntimeVisibilityManager } from "../runtime-visibility";
import DriverHealthSection from "../privacy/DriverHealthSection";
import { useStartupDrivers } from "./useStartupDrivers";

type ManagerTab = "startup" | "users" | "tasks" | "services" | "conceal";

export function StartupDriverTools() {
  const tools = useStartupDrivers();
  const [managerTab, setManagerTab] = useState<ManagerTab>("startup");
  const startupScanLabel = tools.startup ? "Rescan startup impact" : "Scan startup impact";
  const driverScanLabel = tools.drivers ? "Rescan drivers" : "Scan drivers";
  return <div className="flex flex-col gap-4">
    <Card>
      <CardHeader>
        <CardTitle>System Managers</CardTitle>
        <CardDescription>Startup entries, local users, scheduled tasks, services, and runtime concealment, all in one place. The startup impact scan adds launch-path, signer, and conservative keep/review guidance.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex flex-wrap gap-2" role="tablist" aria-label="System managers">
          <Button size="sm" variant={managerTab === "startup" ? "primary" : "outline"} onClick={() => setManagerTab("startup")}><Icon icon="play" />Startup</Button>
          <Button size="sm" variant={managerTab === "users" ? "primary" : "outline"} onClick={() => setManagerTab("users")}><Icon icon="people" />Users</Button>
          <Button size="sm" variant={managerTab === "tasks" ? "primary" : "outline"} onClick={() => setManagerTab("tasks")}><Icon icon="time" />Tasks</Button>
          <Button size="sm" variant={managerTab === "services" ? "primary" : "outline"} onClick={() => setManagerTab("services")}><Icon icon="settings" />Services</Button>
          <Button size="sm" variant={managerTab === "conceal" ? "primary" : "outline"} onClick={() => setManagerTab("conceal")}><Icon icon="eye-off" />Conceal</Button>
        </div>
        {managerTab === "startup" && <>
          <div className="flex flex-wrap gap-2"><Button size="icon" variant="primary" disabled={tools.busy} onClick={() => void tools.scanStartup()} title={startupScanLabel} aria-label={startupScanLabel}><Icon icon={tools.busy || tools.startup ? "refresh" : "search"} className={tools.busy ? "animate-spin" : undefined} /></Button></div>
          <StartupManager embedded />
          {tools.startup?.entries.map((entry) => <div key={entry.id} className="flex flex-wrap items-start gap-3 rounded-[var(--r)] border border-[var(--border)] p-3"><div className="min-w-0 flex-1"><p className="text-sm text-[var(--text)]">{entry.name}</p><p className="break-all font-mono text-[11px] text-[var(--text-mute)]">{entry.command}</p><p className="text-xs text-[var(--text-dim)]">{entry.signer ?? entry.signatureStatus} · {entry.source}</p></div><Badge tone={entry.recommendation === "keep" ? "success" : "warning"}>{entry.recommendation}</Badge></div>)}
          {tools.startup && !tools.startup.entries.length && <Empty text="No startup entries found." />}
        </>}
        {managerTab === "users" && <LocalUsersManager embedded />}
        {managerTab === "tasks" && <ScheduledTasksManager embedded />}
        {managerTab === "services" && <ServiceManager embedded />}
        {managerTab === "conceal" && <RuntimeVisibilityManager embedded />}
      </CardContent>
    </Card>
    {tools.error && <Card><CardContent className="flex items-center gap-3 py-4"><Badge tone="danger">error</Badge><p className="text-sm text-[var(--text-dim)]">{tools.error}</p></CardContent></Card>}
    <Card><CardHeader><CardTitle>Driver inventory</CardTitle><CardDescription>Bounded signed-PnP inventory. Cleanup is intentionally refused because active, offline-device, and rollback safety cannot be proven from inventory alone.</CardDescription></CardHeader><CardContent className="flex flex-col gap-3"><div className="flex flex-wrap gap-2"><Button size="icon" variant="primary" disabled={tools.busy} onClick={() => void tools.scanDrivers()} title={driverScanLabel} aria-label={driverScanLabel}><Icon icon={tools.busy || tools.drivers ? "refresh" : "search"} className={tools.busy ? "animate-spin" : undefined} /></Button><Button variant="outline" onClick={() => void tools.openUpdates()}><Icon icon="arrow-right" /> Open Windows Update</Button>{tools.drivers && <Badge tone="accent">{tools.drivers.drivers.length} drivers</Badge>}</div>{tools.drivers?.drivers.slice(0, 200).map((driver, index) => <div key={`${driver.deviceId}-${driver.infName}-${index}`} className="grid gap-1 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 md:grid-cols-3"><span className="text-sm text-[var(--text)]">{driver.deviceName ?? "Unknown device"}</span><span className="font-mono text-xs text-[var(--text-dim)]">{driver.driverVersion ?? "unknown version"}</span><span className="text-xs text-[var(--text-mute)]">{driver.manufacturer ?? driver.signer ?? "unknown signer"} · {driver.isSigned ? "signed" : "signature unknown"}</span></div>)}{tools.drivers && !tools.drivers.drivers.length && <Empty text="No PnP drivers returned." />}{tools.drivers && <p className="text-xs text-[var(--text-mute)]">{tools.drivers.cleanupLimitation}</p>}</CardContent></Card>
    <DriverHealthSection />
  </div>;
}

function Empty({ text }: { text: string }) { return <p className="py-6 text-center text-sm text-[var(--text-mute)]">{text}</p>; }
