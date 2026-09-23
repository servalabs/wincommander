import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import { Checkbox } from "../../components/ui/bp";
import { Spinner } from "../../components/ui/spinner";
import { useBackend } from "../../hooks/useBackend";
import { releasePackageOperation, tryAcquirePackageOperation } from "../../lib/packageOperationLock";
import { useAppState } from "../../context/AppContext";
import AppIcon from "./components/AppIcon";
import { collectUnifiedPackageUpdates, refreshPackageAndAppInventories } from "./packageUpdateDisplay";
import { getPackageUpdateInventorySnapshot, runPackageUpdateInventoryCheck, subscribeToPackageUpdateInventory } from "../../lib/packageUpdateInventoryStore";

// Display labels for the manager ids the backend reports (package_updates.rs
// `Manager::label`) — always winget/chocolatey/scoop/npm, in that order.
// npm keeps its lowercase brand casing.
const MANAGER_LABELS: Record<string, string> = { winget: "Winget", chocolatey: "Chocolatey", scoop: "Scoop", npm: "npm" };

/**
 * The single multi-manager update executor. Packages & Apps is its only
 * renderer; Maintenance deliberately exposes a handoff instead of a second
 * executor. The file used to live under src/panels/maintenance/, which made it
 * read like duplicated update UI — moved here to sit with its renderer.
 */
export function PackageUpdateTools() {
  const backend = useBackend();
  const { appInventory, runAppInventoryScan, waitForAppInventoryScan } = useAppState();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const packageSnapshot = useSyncExternalStore(
    subscribeToPackageUpdateInventory,
    getPackageUpdateInventorySnapshot,
    getPackageUpdateInventorySnapshot,
  );
  const packages = packageSnapshot.inventory;
  const [packageIds, setPackageIds] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [checkingManagers, setCheckingManagers] = useState(false);
  const [applyingUpdateId, setApplyingUpdateId] = useState<string>();
  const [message, setMessage] = useState<string>();
  const updateRows = useMemo(
    () => collectUnifiedPackageUpdates(packages?.managers ?? [], packageSnapshot.catalogInventoryFresh ? appInventory : null),
    [appInventory, packageSnapshot.catalogInventoryFresh, packages],
  );
  const displayedUpdateIds = useMemo(
    () => new Set(updateRows.map(({ key }) => key)),
    [updateRows],
  );
  const isBusy = busy || packageSnapshot.status === "checking";

  const refreshAppInventoryFully = async () => {
    await runAppInventoryScan(true);
    await waitForAppInventoryScan();
  };

  useEffect(() => {
    setPackageIds((selected) => {
      const visibleSelections = new Set([...selected].filter((id) => displayedUpdateIds.has(id)));
      return visibleSelections.size === selected.size ? selected : visibleSelections;
    });
  }, [displayedUpdateIds]);

  const inspectPackages = async () => {
    if (!tryAcquirePackageOperation()) { setMessage("Another package-manager operation is already running."); return; }
    setBusy(true); setMessage("Refreshing installed apps…");
    try {
      const result = await runPackageUpdateInventoryCheck(() => refreshPackageAndAppInventories(
        async () => {
          setMessage("Refreshing installed app inventory…");
          await refreshAppInventoryFully();
        },
        async () => {
          setMessage("Checking Winget, Chocolatey, Scoop, and npm…");
          setCheckingManagers(true);
          try { return await backendRef.current.packageUpdatesInventory(); }
          finally { setCheckingManagers(false); }
        },
      ));
      setPackageIds(new Set());
      setMessage(result.cancelled
        ? "App inventory refreshed; package manager check was cancelled."
        : "App inventory and package manager updates refreshed.");
    }
    catch (cause) { setMessage(`Update check failed: ${String(cause)}`); }
    finally { setCheckingManagers(false); setBusy(false); releasePackageOperation(); }
  };
  const applyPackages = async (updateKeys = [...packageIds]) => {
    const selectedRows = updateRows.filter(({ key }) => updateKeys.includes(key));
    if (!selectedRows.length) return;
    if (!tryAcquirePackageOperation()) { setMessage("Another package-manager operation is already running."); return; }
    setApplyingUpdateId(selectedRows.length === 1 ? selectedRows[0].key : undefined);
    setBusy(true); setMessage(undefined);
    try {
      let updatedCount = 0;
      const errors: string[] = [];
      const catalogRows = selectedRows.filter((row) => row.kind === "catalog");
      const managerRows = selectedRows.filter((row) => row.kind === "manager");

      for (const row of catalogRows) {
        const result = await backendRef.current.upgradeApp(row.actionId);
        if (result.success) updatedCount += 1;
        else errors.push(result.error || `Could not update ${row.packageName}.`);
      }
      if (managerRows.length) {
        const result = await backendRef.current.packageUpdatesApply(managerRows.map((row) => row.actionId));
        updatedCount += result.updated;
        errors.push(...result.errors);
        if (result.cancelled) errors.push("Package updates were cancelled.");
      }

      setMessage(`Updated ${updatedCount} package${updatedCount === 1 ? "" : "s"}${errors.length ? `; ${errors.length} failed.` : "."} Refreshing the list…`);
      await runPackageUpdateInventoryCheck(() => refreshPackageAndAppInventories(
        refreshAppInventoryFully,
        async () => {
          setCheckingManagers(true);
          try { return await backendRef.current.packageUpdatesInventory(); }
          finally { setCheckingManagers(false); }
        },
      ));
      setPackageIds(new Set());
      setMessage(`Updated ${updatedCount} package${updatedCount === 1 ? "" : "s"}${errors.length ? `; ${errors.length} failed.` : "."} App inventory refreshed.`);
    } catch (cause) { setMessage(`Update operation failed: ${String(cause)}`); }
    finally { setCheckingManagers(false); setApplyingUpdateId(undefined); setBusy(false); releasePackageOperation(); }
  };
  const cancel = async () => { await backendRef.current.packageUpdatesCancel(); };

  const displayedUpdateCount = updateRows.length;
  const managerErrors = packages?.managers.filter((manager) => !manager.available || manager.error) ?? [];

  return <section id="package-updates" className="flex scroll-mt-4 flex-col gap-4">
    <Card>
      <CardHeader><CardTitle>App and package updates</CardTitle><CardDescription>WinGet inventory updates and updates found by Winget, Chocolatey, Scoop, and npm appear together below.</CardDescription></CardHeader>
      <CardContent className="flex flex-wrap items-center gap-2">
        <Button variant="outline" disabled={isBusy} onClick={() => void inspectPackages()} aria-label="Refresh app and package updates" title="Refresh app and package updates"><Icon icon="refresh" />Refresh</Button>
        {isBusy && <span className="flex items-center gap-2 text-sm text-[var(--text-dim)]"><Spinner size={14} />Checking package updates…</span>}
        {isBusy && checkingManagers && <Button variant="outline" onClick={() => void cancel()}><Icon icon="stop" />Cancel check</Button>}
        {packageSnapshot.lastCheckedAt && <span className="text-xs text-[var(--text-mute)]">Last checked {new Date(packageSnapshot.lastCheckedAt).toLocaleTimeString()}</span>}
        {displayedUpdateCount > 0 && <Badge tone="accent">{displayedUpdateCount} update{displayedUpdateCount === 1 ? "" : "s"}</Badge>}
      </CardContent>
    </Card>
    {packageSnapshot.error && <Notice tone="warning" text={`Package update check failed: ${packageSnapshot.error}`} />}
    {managerErrors.length > 0 && <Card><CardContent className="flex flex-wrap gap-x-4 gap-y-1 py-3 text-xs text-[var(--text-mute)]">{managerErrors.map((manager) => <span key={manager.manager}>{MANAGER_LABELS[manager.manager] ?? manager.manager}: {manager.error || "Unavailable"}</span>)}</CardContent></Card>}
    {updateRows.length > 0 ? <div className="app-group-grid app-group-grid--updates">
      {updateRows.map((row) => <PackageUpdateRow
        key={row.key}
        manager={MANAGER_LABELS[row.manager.toLowerCase()] ?? row.manager}
        packageName={row.packageName}
        currentVersion={row.currentVersion}
        availableVersion={row.availableVersion}
        checked={packageIds.has(row.key)}
        disabled={isBusy}
        applying={applyingUpdateId === row.key}
        onToggle={() => setPackageIds((selected) => toggle(selected, row.key))}
        onApply={() => void applyPackages([row.key])}
      />)}
    </div> : packages && <Notice tone="success" text="No updates are available from app inventory, Winget, Chocolatey, Scoop, or npm." />}
    {!packages && packageSnapshot.status === "idle" && !updateRows.length && <Card><CardContent className="py-4 text-sm text-[var(--text-dim)]">The update check runs automatically after WinCommander starts.</CardContent></Card>}
    {!!packageIds.size && <div className="flex justify-end"><Button variant="primary" disabled={isBusy} onClick={() => void applyPackages()}>Update {packageIds.size} selected</Button></div>}
    {message && <Notice tone={message.includes("failed") || message.includes("could not") ? "warning" : "success"} text={message} />}
  </section>;
}

function PackageUpdateRow({ manager, packageName, currentVersion, availableVersion, checked, disabled, applying, onToggle, onApply }: {
  manager: string;
  packageName: string;
  currentVersion: string;
  availableVersion: string;
  checked: boolean;
  disabled: boolean;
  applying: boolean;
  onToggle: () => void;
  onApply: () => void;
}) {
  return <div
    className={`app-card app-card--upgrade update-available ${checked ? "selected" : ""}`}
    role="button"
    tabIndex={disabled ? -1 : 0}
    aria-pressed={checked}
    aria-label={`${checked ? "Deselect" : "Select"} ${packageName} update from ${manager}`}
    onClick={(event) => {
      if ((event.target as HTMLElement).closest("button, label")) return;
      if (!disabled) onToggle();
    }}
    onKeyDown={(event) => {
      if (event.target !== event.currentTarget || disabled) return;
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        onToggle();
      }
    }}
  >
    <span className="app-checkbox-wrap" onClick={(event) => event.stopPropagation()}>
      <Checkbox checked={checked} onChange={onToggle} className="app-checkbox" disabled={disabled} ariaLabel={`Select ${packageName} update`} />
    </span>
    <AppIcon id={packageName} category="misc" />
    <div className="app-info">
      <span className="app-name app-name--truncate" title={packageName}>{packageName}</span>
      <span className="app-description">Detected by {manager} package inventory</span>
      <span className="app-version mono">{currentVersion} → {availableVersion}</span>
    </div>
    <Button
      variant="ghost"
      size="icon"
      className="app-update-btn app-card-action--update size-8"
      onClick={(event) => { event.stopPropagation(); onApply(); }}
      disabled={disabled}
      aria-label={`Update ${packageName} with ${manager}`}
      title={`Update ${packageName}`}
    >
      {applying ? <Spinner size={14} /> : <Icon icon="refresh" />}
    </Button>
  </div>;
}
function Notice({ tone, text }: { tone: "success" | "warning"; text: string }) { return <Card><CardContent className="flex items-center gap-3 py-4"><Badge tone={tone}>{tone}</Badge><p className="text-sm text-[var(--text-dim)]">{text}</p></CardContent></Card>; }
function toggle(current: Set<string>, id: string) { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; }
