import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import StartupManager from "../../components/tweaks/managers/StartupManager";
import DriverHealthSection from "../privacy/DriverHealthSection";
import { useStartupDrivers } from "./useStartupDrivers";

export function StartupDriverTools() {
  const tools = useStartupDrivers();
  const startupScanLabel = tools.startup ? "Rescan startup impact" : "Scan startup impact";
  const driverScanLabel = tools.drivers ? "Rescan drivers" : "Scan drivers";
  return <div className="flex flex-col gap-4">
    <Card><CardHeader><CardTitle>Startup apps</CardTitle><CardDescription>Manage startup entries here. The impact scan adds launch-path, signer, and conservative keep/review guidance.</CardDescription></CardHeader><CardContent className="flex flex-col gap-3"><div className="flex flex-wrap gap-2"><Button size="icon" variant="primary" disabled={tools.busy} onClick={() => void tools.scanStartup()} title={startupScanLabel} aria-label={startupScanLabel}><Icon icon={tools.busy || tools.startup ? "refresh" : "search"} className={tools.busy ? "animate-spin" : undefined} /></Button></div><StartupManager embedded />{tools.startup?.entries.map((entry) => <div key={entry.id} className="flex flex-wrap items-start gap-3 rounded-[var(--r)] border border-[var(--border)] p-3"><div className="min-w-0 flex-1"><p className="text-sm text-[var(--text)]">{entry.name}</p><p className="break-all font-mono text-[11px] text-[var(--text-mute)]">{entry.command}</p><p className="text-xs text-[var(--text-dim)]">{entry.signer ?? entry.signatureStatus} · {entry.source}</p></div><Badge tone={entry.recommendation === "keep" ? "success" : "warning"}>{entry.recommendation}</Badge></div>)}{tools.startup && !tools.startup.entries.length && <Empty text="No startup entries found." />}</CardContent></Card>
    {tools.error && <Card><CardContent className="flex items-center gap-3 py-4"><Badge tone="danger">error</Badge><p className="text-sm text-[var(--text-dim)]">{tools.error}</p></CardContent></Card>}
    <Card><CardHeader><CardTitle>Driver inventory</CardTitle><CardDescription>Bounded signed-PnP inventory. Cleanup is intentionally refused because active, offline-device, and rollback safety cannot be proven from inventory alone.</CardDescription></CardHeader><CardContent className="flex flex-col gap-3"><div className="flex flex-wrap gap-2"><Button size="icon" variant="primary" disabled={tools.busy} onClick={() => void tools.scanDrivers()} title={driverScanLabel} aria-label={driverScanLabel}><Icon icon={tools.busy || tools.drivers ? "refresh" : "search"} className={tools.busy ? "animate-spin" : undefined} /></Button><Button variant="outline" onClick={() => void tools.openUpdates()}><Icon icon="arrow-right" /> Open Windows Update</Button>{tools.drivers && <Badge tone="accent">{tools.drivers.drivers.length} drivers</Badge>}</div>{tools.drivers?.drivers.slice(0, 200).map((driver, index) => <div key={`${driver.deviceId}-${driver.infName}-${index}`} className="grid gap-1 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 md:grid-cols-3"><span className="text-sm text-[var(--text)]">{driver.deviceName ?? "Unknown device"}</span><span className="font-mono text-xs text-[var(--text-dim)]">{driver.driverVersion ?? "unknown version"}</span><span className="text-xs text-[var(--text-mute)]">{driver.manufacturer ?? driver.signer ?? "unknown signer"} · {driver.isSigned ? "signed" : "signature unknown"}</span></div>)}{tools.drivers && !tools.drivers.drivers.length && <Empty text="No PnP drivers returned." />}{tools.drivers && <p className="text-xs text-[var(--text-mute)]">{tools.drivers.cleanupLimitation}</p>}</CardContent></Card>
    <DriverHealthSection />
  </div>;
}

function Empty({ text }: { text: string }) { return <p className="py-6 text-center text-sm text-[var(--text-mute)]">{text}</p>; }
