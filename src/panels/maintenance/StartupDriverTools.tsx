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
import "./StartupDriverTools.css";

type ManagerTab = "startup" | "users" | "tasks" | "services" | "conceal";

const MANAGER_TAB_ORDER: ManagerTab[] = ["startup", "users", "tasks", "services", "conceal"];

export function StartupDriverTools() {
  const tools = useStartupDrivers();
  const [managerTab, setManagerTab] = useState<ManagerTab>("startup");
  const [showAllDrivers, setShowAllDrivers] = useState(false);
  const [isStartupImpactOpen, setIsStartupImpactOpen] = useState(false);
  const [driverScanKey, setDriverScanKey] = useState(0);
  const [managerScanKey, setManagerScanKey] = useState(0);

  const scanChecks = async () => {
    await tools.scanChecks();
    setDriverScanKey((current) => current + 1);
    setManagerScanKey((current) => current + 1);
  };

  const toggleAllDrivers = () => {
    if (showAllDrivers) {
      setShowAllDrivers(false);
      return;
    }
    setShowAllDrivers(true);
    if (!tools.drivers) void scanChecks();
  };
  const moveManagerTab = (key: string) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(key)) return;
    const current = MANAGER_TAB_ORDER.indexOf(managerTab);
    const next = key === 'Home' ? 0 : key === 'End' ? MANAGER_TAB_ORDER.length - 1
      : (current + (key === 'ArrowRight' ? 1 : MANAGER_TAB_ORDER.length - 1)) % MANAGER_TAB_ORDER.length;
    const nextTab = MANAGER_TAB_ORDER[next];
    setManagerTab(nextTab);
    window.requestAnimationFrame(() => document.getElementById(`maintenance-manager-tab-${nextTab}`)?.focus());
  };

  return <div className="flex flex-col gap-4">
    <Card>
      <CardHeader className="gap-3">
        <div className="system-manager-header-row">
          <CardTitle>System Managers</CardTitle>
          <div className={`manager-tab-switch manager-tab-switch--${MANAGER_TAB_ORDER.indexOf(managerTab)}`} role="tablist" aria-label="System managers">
            <span className="manager-tab-switch__thumb" aria-hidden="true" />
            <ManagerTabButton active={managerTab === "startup"} icon="play" label="Startup" onClick={() => setManagerTab("startup")} onNavigate={moveManagerTab} />
            <ManagerTabButton active={managerTab === "users"} icon="people" label="Users" onClick={() => setManagerTab("users")} onNavigate={moveManagerTab} />
            <ManagerTabButton active={managerTab === "tasks"} icon="time" label="Tasks" onClick={() => setManagerTab("tasks")} onNavigate={moveManagerTab} />
            <ManagerTabButton active={managerTab === "services"} icon="settings" label="Services" onClick={() => setManagerTab("services")} onNavigate={moveManagerTab} />
            <ManagerTabButton active={managerTab === "conceal"} icon="eye-off" label="Conceal" onClick={() => setManagerTab("conceal")} onNavigate={moveManagerTab} />
          </div>
          <Button size="icon" variant="primary" className="system-manager-scan" disabled={tools.busy} onClick={() => void scanChecks()} title={tools.busy ? "Scanning system managers" : "Scan system managers"} aria-label={tools.busy ? "Scanning system managers" : "Scan system managers"}>
            <Icon icon={tools.busy ? "refresh" : "search"} className={tools.busy ? "animate-spin" : undefined} />
          </Button>
        </div>
        <CardDescription>Startup entries, users, tasks, services, runtime visibility, and drivers are scanned once here. Switching views reuses those results until you scan again.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div id="maintenance-manager-panel-startup" role="tabpanel" aria-labelledby="maintenance-manager-tab-startup" hidden={managerTab !== "startup"}>
          <StartupManager embedded scanKey={managerScanKey} />
          {tools.startup && <section className="startup-impact-section" aria-label="Signature and launch-impact review">
            <button type="button" className="startup-impact-section__toggle" aria-expanded={isStartupImpactOpen} onClick={() => setIsStartupImpactOpen((open) => !open)}>
              <span className="startup-impact-section__heading"><span className="text-xs font-semibold uppercase tracking-wide text-[var(--text-dim)]">Signature &amp; launch-impact review</span><span className="text-xs text-[var(--text-mute)]">Unsigned or unverifiable launchers</span></span>
              <Badge tone="accent">{tools.startup.entries.length} entries</Badge>
              <Icon icon={isStartupImpactOpen ? "chevron-up" : "chevron-down"} size={16} aria-hidden="true" />
            </button>
            {isStartupImpactOpen && <div className="startup-impact-section__list">
              {tools.startup.entries.map((entry) => <div key={entry.id} className="startup-impact-row"><div className="min-w-0"><p className="startup-impact-row__name">{entry.name}</p><p className="startup-impact-row__command" title={entry.command}>{entry.command}</p><p className="text-xs text-[var(--text-dim)]">{entry.signer ?? entry.signatureStatus} · {entry.source}</p></div><Badge tone={entry.recommendation === "keep" ? "success" : "warning"}>{entry.recommendation}</Badge></div>)}
              {!tools.startup.entries.length && <Empty text="No startup entries found." />}
            </div>}
          </section>}
        </div>
        <div id="maintenance-manager-panel-users" role="tabpanel" aria-labelledby="maintenance-manager-tab-users" hidden={managerTab !== "users"}><LocalUsersManager embedded scanKey={managerScanKey} /></div>
        <div id="maintenance-manager-panel-tasks" role="tabpanel" aria-labelledby="maintenance-manager-tab-tasks" hidden={managerTab !== "tasks"}><ScheduledTasksManager embedded scanKey={managerScanKey} /></div>
        <div id="maintenance-manager-panel-services" role="tabpanel" aria-labelledby="maintenance-manager-tab-services" hidden={managerTab !== "services"}><ServiceManager embedded scanKey={managerScanKey} /></div>
        <div id="maintenance-manager-panel-conceal" role="tabpanel" aria-labelledby="maintenance-manager-tab-conceal" hidden={managerTab !== "conceal"}><RuntimeVisibilityManager embedded scanKey={managerScanKey} /></div>
      </CardContent>
    </Card>

    {tools.error && <Card role="alert"><CardContent className="flex items-center gap-3 py-4"><Badge tone="danger">error</Badge><p className="text-sm text-[var(--text-dim)]">{tools.error}</p></CardContent></Card>}

    <Card>
      <CardHeader className="gap-2">
        <div className="flex flex-wrap items-center justify-between gap-3"><div><CardTitle>Drivers</CardTitle><CardDescription>See issues first; expand the inventory only when you need to inspect every installed driver.</CardDescription></div><div className="flex items-center gap-2"><Button size="sm" variant="outline" onClick={() => void tools.openUpdates()}><Icon icon="arrow-right" />Optional updates</Button><Button size="sm" variant="outline" disabled={tools.busy} onClick={toggleAllDrivers}>{showAllDrivers ? "Hide all drivers" : "Show all drivers"}</Button></div></div>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <DriverHealthSection embedded hideActions scanKey={driverScanKey} />
        {showAllDrivers && <div className="driver-inventory-section"><div className="driver-inventory-section__heading"><div><p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-dim)]">All installed drivers</p><p className="text-xs text-[var(--text-mute)]">This list is informational; driver removal is intentionally not offered here.</p></div>{tools.drivers && <Badge tone="accent">{tools.drivers.drivers.length} drivers</Badge>}</div><div className="max-h-[56vh] overflow-auto flex flex-col gap-2">{tools.drivers?.drivers.slice(0, 200).map((driver, index) => <DriverRow key={`${driver.deviceId}-${driver.infName}-${index}`} driver={driver} />)}{tools.drivers && !tools.drivers.drivers.length && <Empty text="No PnP drivers returned." />}{!tools.drivers && <Empty text="Loading the current driver inventory…" />}</div>{tools.drivers && <p className="text-xs text-[var(--text-mute)]">{tools.drivers.cleanupLimitation}</p>}</div>}
      </CardContent>
    </Card>
  </div>;
}

function ManagerTabButton({ active, icon, label, onClick, onNavigate }: { active: boolean; icon: string; label: string; onClick: () => void; onNavigate: (key: string) => void }) {
  const id = label.toLowerCase();
  return <Button size="sm" variant="ghost" id={`maintenance-manager-tab-${id}`} role="tab" tabIndex={active ? 0 : -1} aria-selected={active} aria-controls={`maintenance-manager-panel-${id}`} className={`manager-tab-switch__btn${active ? " manager-tab-switch__btn--active" : ""}`} onClick={onClick} onKeyDown={(event) => { if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) { event.preventDefault(); onNavigate(event.key); } }}><Icon icon={icon} />{label}</Button>;
}

function DriverRow({ driver }: { driver: { deviceName: string | null; deviceClass: string | null; manufacturer: string | null; signer: string | null; driverVersion: string | null; driverDate: string | null; isSigned: boolean | null } }) {
  const signed = driver.isSigned === true;
  return <div className={`driver-inventory-row${signed ? " is-signed" : " is-unsigned"}`}><div className="min-w-0"><p className="text-sm text-[var(--text)]">{driver.deviceName ?? "Unknown device"}</p><p className="text-xs text-[var(--text-mute)]">{driver.deviceClass ?? "Device driver"} · {driver.manufacturer ?? driver.signer ?? "Unknown publisher"}</p></div><p className="font-mono text-xs text-[var(--text-dim)]">{driver.driverVersion ?? "Unknown version"}{driver.driverDate ? ` · ${driver.driverDate}` : ""}</p><span className="driver-signature-state"><Icon icon={signed ? "check" : "cross"} size={14} />{signed ? "Signed" : "Unsigned"}</span></div>;
}

function Empty({ text }: { text: string }) { return <p className="py-6 text-center text-sm text-[var(--text-mute)]">{text}</p>; }
