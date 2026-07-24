import { useRef, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import type { ManagerInventory, PackageUpdateInventory } from "../../hooks/useBackend";
import { useBackend } from "../../hooks/useBackend";
import { releasePackageOperation, tryAcquirePackageOperation } from "../../lib/packageOperationLock";

/**
 * The single multi-manager update executor. It is rendered in Packages & Apps;
 * Maintenance deliberately exposes a handoff instead of another executor.
 */
export function PackageUpdateTools() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;
  const [packages, setPackages] = useState<PackageUpdateInventory>();
  const [packageIds, setPackageIds] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string>();

  const inspectPackages = async () => {
    if (!tryAcquirePackageOperation()) { setMessage("Another package-manager operation is already running."); return; }
    setBusy(true); setMessage(undefined);
    try { setPackages(await backendRef.current.packageUpdatesInventory()); setPackageIds(new Set()); }
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
      setPackages(await backendRef.current.packageUpdatesInventory()); setPackageIds(new Set());
    } catch (cause) { setMessage(String(cause)); }
    finally { setBusy(false); releasePackageOperation(); }
  };
  const cancel = async () => { await backendRef.current.packageUpdatesCancel(); };

  return <section id="package-updates" className="flex scroll-mt-4 flex-col gap-4">
    <Card>
      <CardHeader><CardTitle>Updates across package managers</CardTitle><CardDescription>Check and apply explicit updates from Winget, Chocolatey, Scoop, and global npm. Each manager reports availability independently.</CardDescription></CardHeader>
      <CardContent className="flex flex-wrap items-center gap-2"><Button variant="primary" disabled={busy} onClick={() => void inspectPackages()}><Icon icon="search" />{busy ? "Checking…" : "Check updates"}</Button>{busy && <Button variant="outline" onClick={() => void cancel()}><Icon icon="stop" /> Cancel</Button>}{packages && <Badge tone="accent">{packages.managers.reduce((count, manager) => count + manager.updates.length, 0)} available</Badge>}</CardContent>
    </Card>
    {packages?.managers.map((manager) => <PackageManager key={manager.manager} manager={manager} selected={packageIds} toggle={(id) => setPackageIds(toggle(packageIds, id))} />)}
    {!!packageIds.size && <div className="flex justify-end"><Button variant="primary" disabled={busy} onClick={() => void applyPackages()}>Update {packageIds.size} selected</Button></div>}
    {message && <Notice tone={message.includes("failed") ? "warning" : "success"} text={message} />}
  </section>;
}

function PackageManager({ manager, selected, toggle: onToggle }: { manager: ManagerInventory; selected: Set<string>; toggle: (id: string) => void }) {
  if (!manager.available) return <Notice tone="warning" text={`${manager.manager}: ${manager.error ?? "not available"}`} />;
  if (manager.error) return <Notice tone="warning" text={`${manager.manager}: ${manager.error}`} />;
  return <Card><CardHeader><CardTitle>{manager.manager}</CardTitle><CardDescription>{manager.updates.length ? "Select the updates to apply." : "No updates reported."}</CardDescription></CardHeader>{!!manager.updates.length && <CardContent className="flex flex-col gap-2">{manager.updates.map((item) => <SelectableRow key={item.id} checked={selected.has(item.id)} onClick={() => onToggle(item.id)} title={item.package} detail={`${item.currentVersion} → ${item.availableVersion}`} />)}</CardContent>}</Card>;
}

function SelectableRow({ checked, onClick, title, detail }: { checked: boolean; onClick: () => void; title: string; detail: string }) {
  return <button type="button" onClick={onClick} className="flex w-full items-start gap-3 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 text-left hover:bg-[var(--surface-2)]"><span className={`mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-[var(--r-sm)] border ${checked ? "border-[var(--accent)] bg-[var(--accent)] text-[var(--accent-contrast)]" : "border-[var(--border-strong)]"}`}>{checked && <Icon icon="check" />}</span><span className="min-w-0"><span className="block text-sm text-[var(--text)]">{title}</span><span className="block break-all font-mono text-[11px] text-[var(--text-mute)]">{detail}</span></span></button>;
}
function Notice({ tone, text }: { tone: "success" | "warning"; text: string }) { return <Card><CardContent className="flex items-center gap-3 py-4"><Badge tone={tone}>{tone}</Badge><p className="text-sm text-[var(--text-dim)]">{text}</p></CardContent></Card>; }
function toggle(current: Set<string>, id: string) { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; }
