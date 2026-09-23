import { useEffect, useMemo, useRef, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import { Checkbox } from "../../components/ui/bp";
import { Spinner } from "../../components/ui/spinner";
import type { PackageUpdateInventory } from "../../hooks/useBackend";
import { useBackend } from "../../hooks/useBackend";
import { releasePackageOperation, tryAcquirePackageOperation } from "../../lib/packageOperationLock";
import { useAppState } from "../../context/AppContext";
import AppIcon from "./components/AppIcon";
import { collectManagerUpdates, managerUpdateStatus, refreshPackageAndAppInventories } from "./packageUpdateDisplay";

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
  const { appInventory, runAppInventoryScan } = useAppState();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [packages, setPackages] = useState<PackageUpdateInventory>();
  const [packageIds, setPackageIds] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [checkingManagers, setCheckingManagers] = useState(false);
  const [applyingUpdateId, setApplyingUpdateId] = useState<string>();
  const [message, setMessage] = useState<string>();
  const updateRows = useMemo(
    () => packages ? collectManagerUpdates(packages.managers, appInventory) : [],
    [appInventory, packages],
  );
  const displayedUpdateIds = useMemo(
    () => new Set(updateRows.map(({ update }) => update.id)),
    [updateRows],
  );

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
      const result = await refreshPackageAndAppInventories(
        async () => {
          setMessage("Refreshing installed app inventory…");
          await runAppInventoryScan(true);
        },
        async () => {
          setMessage("Checking Winget, Chocolatey, Scoop, and npm…");
          setCheckingManagers(true);
          try { return await backendRef.current.packageUpdatesInventory(); }
          finally { setCheckingManagers(false); }
        },
      );
      setPackages(result); setPackageIds(new Set());
      setMessage(result.cancelled
        ? "App inventory refreshed; package manager check was cancelled."
        : "App inventory and package manager updates refreshed.");
    }
    catch (cause) { setMessage(String(cause)); }
    finally { setCheckingManagers(false); setBusy(false); releasePackageOperation(); }
  };
  const applyPackages = async (updateIds = [...packageIds]) => {
    if (!updateIds.length) return;
    if (!tryAcquirePackageOperation()) { setMessage("Another package-manager operation is already running."); return; }
    setApplyingUpdateId(updateIds.length === 1 ? updateIds[0] : undefined);
    setBusy(true); setMessage(undefined);
    try {
      const result = await backendRef.current.packageUpdatesApply(updateIds);
      const updateSummary = result.cancelled
        ? `Package updates cancelled after ${result.updated} update(s).`
        : `Updated ${result.updated} package(s)${result.errors.length ? `; ${result.errors.length} failed.` : "."}`;
      // Reconcile both inventories through the same flow used by Check updates.
      const refreshed = await refreshPackageAndAppInventories(
        () => runAppInventoryScan(true),
        async () => {
          setCheckingManagers(true);
          try { return await backendRef.current.packageUpdatesInventory(); }
          finally { setCheckingManagers(false); }
        },
      );
      setPackages(refreshed); setPackageIds(new Set());
      setMessage(`${updateSummary} App inventory refreshed.`);
    } catch (cause) { setMessage(String(cause)); }
    finally { setCheckingManagers(false); setApplyingUpdateId(undefined); setBusy(false); releasePackageOperation(); }
  };
  const cancel = async () => { await backendRef.current.packageUpdatesCancel(); };

  const displayedUpdateCount = updateRows.length;

  return <section id="package-updates" className="flex scroll-mt-4 flex-col gap-4">
    <Card>
      <CardHeader><CardTitle>App and package updates</CardTitle><CardDescription>One check refreshes the installed app list and checks Winget, Chocolatey, Scoop, and npm. Catalog apps appear once; extra package manager updates use the same cards and actions below.</CardDescription></CardHeader>
      <CardContent className="flex flex-wrap items-center gap-2"><Button variant="primary" disabled={busy} onClick={() => void inspectPackages()}><Icon icon="search" />{busy ? "Checking…" : "Check for updates"}</Button>{checkingManagers && <Button variant="outline" onClick={() => void cancel()}><Icon icon="stop" /> Cancel package check</Button>}{packages && <Badge tone="accent">{displayedUpdateCount} additional update{displayedUpdateCount === 1 ? "" : "s"}</Badge>}</CardContent>
    </Card>
    {packages && <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2" role="group" aria-label="Package manager check results">
        {packages.managers.map((manager) => {
          const status = managerUpdateStatus(manager);
          const label = MANAGER_LABELS[manager.manager] ?? manager.manager;
          return <div key={manager.manager} className="flex flex-wrap items-center gap-1.5 rounded-[var(--r)] border border-[var(--border)] px-2 py-1.5 text-xs" title={manager.error ?? undefined}>
            <span className="text-[var(--text-dim)]">{label}</span><Badge tone={status.tone}>{status.label}</Badge>
            {manager.error && <span className="text-[var(--text-mute)]">{manager.error}</span>}
          </div>;
        })}
      </div>
      {updateRows.length ? <>
        <div className="grid-divider"><div className="divider-line" /><div className="divider-label">OTHER PACKAGES ({updateRows.length})</div><div className="divider-line" /></div>
        <div className="app-group-grid app-group-grid--updates">
          {updateRows.map(({ manager, update }) => <PackageUpdateRow
            key={update.id}
            manager={MANAGER_LABELS[manager.manager] ?? manager.manager}
            packageName={update.package}
            currentVersion={update.currentVersion}
            availableVersion={update.availableVersion}
            checked={packageIds.has(update.id)}
            disabled={busy}
            applying={applyingUpdateId === update.id}
            onToggle={() => setPackageIds((selected) => toggle(selected, update.id))}
            onApply={() => void applyPackages([update.id])}
          />)}
        </div>
      </> : <Notice tone="success" text="No additional package manager updates are available." />}
    </div>}
    {!!packageIds.size && <div className="flex justify-end"><Button variant="primary" disabled={busy} onClick={() => void applyPackages()}>Update {packageIds.size} selected</Button></div>}
    {message && <Notice tone={message.includes("failed") ? "warning" : "success"} text={message} />}
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
