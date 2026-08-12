import { useEffect, useMemo, useRef, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { resolveAvailableTab } from "../../components/ui/tabSelection";
import type { ManagerInventory, PackageUpdateInventory } from "../../hooks/useBackend";
import { executeBackendCommand, useBackend } from "../../hooks/useBackend";
import { releasePackageOperation, tryAcquirePackageOperation } from "../../lib/packageOperationLock";
import { useAppState } from "../../context/AppContext";
import { filterCatalogDuplicates } from "./packageUpdateDisplay";

// Managers that can be installed deliberately from this screen. They are not
// part of the engine readiness grid or its bulk-install action.
const INSTALLABLE_MANAGERS: Record<string, string> = { chocolatey: "chocolatey", scoop: "scoop" };

// Display labels for the manager ids the backend reports (package_updates.rs
// `Manager::label`) — always winget/chocolatey/scoop/npm, in that order.
// npm keeps its lowercase brand casing; the id itself is the tab value.
const MANAGER_LABELS: Record<string, string> = { winget: "Winget", chocolatey: "Chocolatey", scoop: "Scoop", npm: "npm" };

// Tab that opens by default: whichever manager has the most pending updates,
// or the first manager (backend order) if none do.
function pickDefaultManager(managers: ManagerInventory[]): string | undefined {
  if (!managers.length) return undefined;
  return managers.reduce((best, manager) => (manager.updates.length > best.updates.length ? manager : best), managers[0]).manager;
}

/**
 * The single multi-manager update executor. Packages & Apps is its only
 * renderer; Maintenance deliberately exposes a handoff instead of a second
 * executor. The file used to live under src/panels/maintenance/, which made it
 * read like duplicated update UI — moved here to sit with its renderer.
 */
export function PackageUpdateTools() {
  const backend = useBackend();
  const { appInventory, runAppInventoryScan } = useAppState();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [packages, setPackages] = useState<PackageUpdateInventory>();
  const [packageIds, setPackageIds] = useState<Set<string>>(new Set());
  const [activeManager, setActiveManager] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string>();
  const [installingManagers, setInstallingManagers] = useState<Set<string>>(new Set());
  const displayedManagers = useMemo(
    () => packages ? filterCatalogDuplicates(packages.managers, appInventory) : [],
    [appInventory, packages],
  );
  const hiddenUpdateCounts = useMemo(() => new Map(
    (packages?.managers ?? []).map((manager) => {
      const visible = displayedManagers.find((candidate) => candidate.manager === manager.manager);
      return [manager.manager, manager.updates.length - (visible?.updates.length ?? 0)];
    }),
  ), [displayedManagers, packages]);
  const displayedUpdateIds = useMemo(
    () => new Set(displayedManagers.flatMap((manager) => manager.updates.map((update) => update.id))),
    [displayedManagers],
  );
  const visibleActiveManager = resolveAvailableTab(
    displayedManagers.map((manager) => manager.manager),
    activeManager,
    pickDefaultManager(displayedManagers),
  );

  useEffect(() => {
    if (!packages || visibleActiveManager === activeManager) return;
    setActiveManager(visibleActiveManager ?? pickDefaultManager(displayedManagers));
  }, [activeManager, displayedManagers, packages, visibleActiveManager]);

  useEffect(() => {
    setPackageIds((selected) => {
      const visibleSelections = new Set([...selected].filter((id) => displayedUpdateIds.has(id)));
      return visibleSelections.size === selected.size ? selected : visibleSelections;
    });
  }, [displayedUpdateIds]);

  const inspectPackages = async () => {
    if (!tryAcquirePackageOperation()) { setMessage("Another package-manager operation is already running."); return; }
    setBusy(true); setMessage(undefined);
    try {
      const result = await backendRef.current.packageUpdatesInventory();
      setPackages(result); setPackageIds(new Set()); setActiveManager(undefined);
    }
    catch (cause) { setMessage(String(cause)); }
    finally { setBusy(false); releasePackageOperation(); }
  };
  const applyPackages = async () => {
    if (!packageIds.size) return;
    if (!tryAcquirePackageOperation()) { setMessage("Another package-manager operation is already running."); return; }
    setBusy(true); setMessage(undefined);
    try {
      const result = await backendRef.current.packageUpdatesApply([...packageIds]);
      setMessage(result.cancelled ? `Package updates cancelled after ${result.updated} update(s).` : `Updated ${result.updated} package(s)${result.errors.length ? `; ${result.errors.length} failed.` : "."}`);
      // Package-manager update data and the curated app inventory are two
      // different views. Reconcile both after completion so cards immediately
      // reflect real installed/not-installed state without a panel reload.
      await runAppInventoryScan(true);
      setPackages(await backendRef.current.packageUpdatesInventory()); setPackageIds(new Set()); setActiveManager(undefined);
    } catch (cause) { setMessage(String(cause)); }
    finally { setBusy(false); releasePackageOperation(); }
  };
  const cancel = async () => { await backendRef.current.packageUpdatesCancel(); };

  const installManager = async (manager: string) => {
    setInstallingManagers((prev) => new Set(prev).add(manager));
    try {
      const result = await executeBackendCommand<unknown>("Install-Dependency", { Id: INSTALLABLE_MANAGERS[manager] });
      setMessage(result.success ? `${manager} installed.` : (result.error || `Failed to install ${manager}.`));
      if (result.success) {
        await runAppInventoryScan(true);
        await inspectPackages();
      }
    } finally {
      setInstallingManagers((prev) => { const next = new Set(prev); next.delete(manager); return next; });
    }
  };

  const displayedUpdateCount = displayedManagers.reduce((count, manager) => count + manager.updates.length, 0);

  return <section id="package-updates" className="flex scroll-mt-4 flex-col gap-4">
    <Card>
      <CardHeader><CardTitle>Updates across package managers</CardTitle><CardDescription>Check and apply explicit updates from Winget, Chocolatey, Scoop, and global npm. Each manager reports availability independently.</CardDescription></CardHeader>
      <CardContent className="flex flex-wrap items-center gap-2"><Button variant="primary" disabled={busy} onClick={() => void inspectPackages()}><Icon icon="search" />{busy ? "Checking…" : "Check updates"}</Button>{busy && <Button variant="outline" onClick={() => void cancel()}><Icon icon="stop" /> Cancel</Button>}{packages && <Badge tone="accent">{displayedUpdateCount} additional</Badge>}</CardContent>
    </Card>
    {visibleActiveManager && <Tabs value={visibleActiveManager} onValueChange={setActiveManager}>
      <TabsList className="w-full flex-wrap justify-start">{displayedManagers.map((manager) => <TabsTrigger key={manager.manager} value={manager.manager} className="gap-1.5">{MANAGER_LABELS[manager.manager] ?? manager.manager}<Badge tone={manager.updates.length ? "accent" : "neutral"}>{manager.updates.length}</Badge></TabsTrigger>)}</TabsList>
      {displayedManagers.map((manager) => <TabsContent key={manager.manager} value={manager.manager}><PackageManager manager={manager} hiddenUpdateCount={hiddenUpdateCounts.get(manager.manager) ?? 0} selected={packageIds} toggle={(id) => setPackageIds(toggle(packageIds, id))} onInstallManager={installManager} installingManagers={installingManagers} /></TabsContent>)}
    </Tabs>}
    {!!packageIds.size && <div className="flex justify-end"><Button variant="primary" disabled={busy} onClick={() => void applyPackages()}>Update {packageIds.size} selected</Button></div>}
    {message && <Notice tone={message.includes("failed") ? "warning" : "success"} text={message} />}
  </section>;
}

function PackageManager({ manager, hiddenUpdateCount, selected, toggle: onToggle, onInstallManager, installingManagers }: { manager: ManagerInventory; hiddenUpdateCount: number; selected: Set<string>; toggle: (id: string) => void; onInstallManager: (manager: string) => void; installingManagers: Set<string> }) {
  if (!manager.available) {
    // Chocolatey/Scoop are optional package-manager choices. Winget/npm keep
    // the passive notice because this panel cannot install them safely here.
    if (manager.manager in INSTALLABLE_MANAGERS) {
      const installing = installingManagers.has(manager.manager);
      return <Card><CardContent className="flex flex-wrap items-center justify-between gap-3 py-4">
        <p className="text-sm text-[var(--text-dim)]">{manager.manager}: {manager.error ?? "not available"}</p>
        <Button variant="primary" disabled={installing} onClick={() => onInstallManager(manager.manager)}><Icon icon="download" />{installing ? "Installing…" : `Install ${manager.manager}`}</Button>
      </CardContent></Card>;
    }
    return <Notice tone="warning" text={`${manager.manager}: ${manager.error ?? "not available"}`} />;
  }
  if (manager.error) return <Notice tone="warning" text={`${manager.manager}: ${manager.error}`} />;
  return <Card><CardHeader><CardTitle>{manager.manager}</CardTitle><CardDescription>{manager.updates.length ? "Select the additional updates to apply." : hiddenUpdateCount ? "All detected updates are already shown in the catalog lists above." : "No updates reported."}</CardDescription></CardHeader>{!!manager.updates.length && <CardContent className="grid grid-cols-1 gap-2 md:grid-cols-2 2xl:grid-cols-3">{manager.updates.map((item) => <SelectableRow key={item.id} checked={selected.has(item.id)} onClick={() => onToggle(item.id)} title={item.package} detail={`${item.currentVersion} → ${item.availableVersion}`} />)}</CardContent>}</Card>;
}

function SelectableRow({ checked, onClick, title, detail }: { checked: boolean; onClick: () => void; title: string; detail: string }) {
  return <button type="button" onClick={onClick} aria-pressed={checked} aria-label={`${checked ? "Deselect" : "Select"} ${title} update`} className="flex w-full items-start gap-3 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 text-left hover:bg-[var(--surface-2)]"><span className={`mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-[var(--r-sm)] border ${checked ? "border-[var(--accent)] bg-[var(--accent)] text-[var(--accent-contrast)]" : "border-[var(--border-strong)]"}`}>{checked && <Icon icon="check" />}</span><span className="min-w-0"><span className="block text-sm text-[var(--text)]">{title}</span><span className="block break-all font-mono text-[11px] text-[var(--text-mute)]">{detail}</span></span></button>;
}
function Notice({ tone, text }: { tone: "success" | "warning"; text: string }) { return <Card><CardContent className="flex items-center gap-3 py-4"><Badge tone={tone}>{tone}</Badge><p className="text-sm text-[var(--text-dim)]">{text}</p></CardContent></Card>; }
function toggle(current: Set<string>, id: string) { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; }
