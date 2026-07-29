import { useState } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "../../components/ui/alert-dialog";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import { Checkbox } from "../../components/ui/bp";
import { Tabs, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { useBackend } from "../../hooks/useBackend";
import { showSuccess } from "../../utils/toast";
import { useRegistryTools } from "./useRegistryTools";

type Action = "disable" | "enable" | "remove";

export function RegistryTools() {
  const tools = useRegistryTools();
  const { openPath } = useBackend();
  const [pendingAction, setPendingAction] = useState<Action>();
  const entries = tools.tool === "orphans" ? tools.registryScan?.entries : tools.contextScan?.entries;
  const scanLabel = entries ? `Rescan ${tools.tool}` : `Scan ${tools.tool}`;

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <div className="flex flex-wrap items-center gap-3">
            <CardTitle>Registry and Explorer hygiene</CardTitle>
            <Tabs value={tools.tool} onValueChange={(value) => tools.changeTool(value as "orphans" | "context")} className="min-w-0">
              <TabsList><TabsTrigger value="orphans">Registry orphans</TabsTrigger><TabsTrigger value="context">Context menu</TabsTrigger></TabsList>
            </Tabs>
            <div className="ml-auto flex items-center gap-2">
              {entries && <Badge tone="accent">{entries.length} candidate{entries.length === 1 ? "" : "s"}</Badge>}
              <Button size="icon" variant="primary" disabled={tools.busy} onClick={() => void tools.scan()} title={scanLabel} aria-label={scanLabel}><Icon icon={tools.busy || entries ? "refresh" : "search"} className={tools.busy ? "animate-spin" : undefined} /></Button>
            </div>
          </div>
          <CardDescription>Pre-scanned once for this Maintenance session. Windows and WinCommander entries stay protected; use Refresh only when you want a new snapshot.</CardDescription>
        </CardHeader>
      </Card>

      {tools.error && <Notice tone="danger" text={tools.error} />}
      {tools.result && <Notice tone="success" text={`Changed ${"removed" in tools.result ? tools.result.removed : tools.result.changed} entries. ${tools.result.backupLocations.length} registry backups retained.`} />}

      {tools.tool === "orphans" && tools.registryScan && (
        <Card>
          <CardHeader><CardTitle>Registry Orphans</CardTitle><CardDescription>Windows still has a pointer for an app component, but the program file it points to no longer exists. Removing a selected entry clears that stale pointer only; it does not uninstall an app.</CardDescription></CardHeader>
          <CardContent className="grid gap-2 xl:grid-cols-2">
            {tools.registryScan.entries.map((entry) => <RegistryEntryRow key={entry.id} checked={tools.selected.has(entry.id)} onClick={() => tools.select(entry.id)} classId={entry.classId} hive={entry.hive} serverKind={entry.serverKind} missingServer={entry.missingServer} onRevealFolder={(path) => void openParentFolder(openPath, path)} />)}
            {!tools.registryScan.entries.length && <EmptyState />}
          </CardContent>
        </Card>
      )}

      {tools.tool === "context" && tools.contextScan && (
        <Card>
          <CardHeader><CardTitle>Third-party Explorer verbs</CardTitle><CardDescription>Disabled entries remain recoverable; removal keeps an app-owned registry backup. Hover a row for its full command.</CardDescription></CardHeader>
          <CardContent className="grid gap-2 xl:grid-cols-2">
            {tools.contextScan.entries.map((entry) => <ContextVerbRow key={entry.id} checked={tools.selected.has(entry.id)} onClick={() => tools.select(entry.id)} label={entry.label} location={entry.location} command={entry.command} enabled={entry.enabled} />)}
            {!tools.contextScan.entries.length && <EmptyState />}
          </CardContent>
        </Card>
      )}

      {!!tools.selected.size && <div className="flex flex-wrap justify-end gap-2">
        {tools.tool === "context" && <>
          <Button variant="outline" onClick={() => setPendingAction("disable")}>Disable selected</Button>
          <Button variant="outline" onClick={() => setPendingAction("enable")}>Enable selected</Button>
        </>}
        <Button variant="danger" onClick={() => setPendingAction("remove")}>Remove {tools.selected.size} selected</Button>
      </div>}

      <AlertDialog open={pendingAction !== undefined} onOpenChange={(open) => { if (!open) setPendingAction(undefined); }}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>{pendingAction === "remove" ? "Remove" : pendingAction === "enable" ? "Enable" : "Disable"} selected entries?</AlertDialogTitle><AlertDialogDescription>Each target is re-read after preview. Changed or out-of-scope registry data is refused, and destructive operations retain a backup.</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={() => { const action = pendingAction; setPendingAction(undefined); if (action) void tools.mutate(action); }}>Continue</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function RegistryEntryRow({ checked, onClick, classId, hive, serverKind, missingServer, onRevealFolder }: { checked: boolean; onClick: () => void; classId: string; hive: string; serverKind: string; missingServer: string; onRevealFolder: (path: string) => void }) {
  const currentPath = `${hive}\\CLSID\\${classId}`;
  return <LocationRow checked={checked} onClick={onClick} title="Windows points to an app file that is gone" state="Missing app file" tone="warning" currentLabel="Windows registration" currentPath={currentPath} expectedLabel={`${serverKind} path that is missing`} expectedPath={missingServer} onReveal={() => onRevealFolder(missingServer)} />;
}

// One compact row per verb instead of a wrapping name+location+command block --
// the full command (which can be long) sits behind the row's title tooltip.
function ContextVerbRow({ checked, onClick, label, location, command, enabled }: { checked: boolean; onClick: () => void; label: string; location: string; command: string; enabled: boolean }) {
  return <LocationRow checked={checked} onClick={onClick} title={label} state={enabled ? "Active" : "Disabled"} tone={enabled ? "success" : "warning"} currentLabel="Current registry location" currentPath={location} expectedLabel="Command it runs" expectedPath={command} />;
}

function LocationRow({ checked, onClick, title, state, tone, currentLabel, currentPath, expectedLabel, expectedPath, onReveal }: { checked: boolean; onClick: () => void; title: string; state: string; tone: "success" | "warning"; currentLabel: string; currentPath: string; expectedLabel: string; expectedPath: string; onReveal?: () => void }) {
  const copyPath = async (path: string) => {
    const copied = await navigator.clipboard.writeText(path).then(() => true).catch(() => false);
    if (copied) void showSuccess("Full path copied.");
  };
  return <div className="rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface)] p-3 hover:bg-[var(--surface-2)]">
    <div className="flex items-center gap-2">
      <Checkbox checked={checked} onChange={onClick} />
      <button type="button" onClick={onClick} aria-pressed={checked} className="min-w-0 flex-1 text-left">
        <span className="truncate text-sm font-medium text-[var(--text)]">{title}</span>
      </button>
      <Badge tone={tone}>{state}</Badge>
    </div>
    <PathDetail icon="database" label={currentLabel} path={currentPath} onCopy={() => void copyPath(currentPath)} />
    <PathDetail icon="folder-open" label={expectedLabel} path={expectedPath} onCopy={() => void copyPath(expectedPath)} onReveal={onReveal} />
  </div>;
}

function openParentFolder(openPath: (path: string) => Promise<unknown>, path: string) {
  const normalized = path.replace(/\//g, "\\");
  const separator = normalized.lastIndexOf("\\");
  const folder = separator > 0 ? normalized.slice(0, separator) : normalized;
  return openPath(folder).catch(() => {});
}

function PathDetail({ icon, label, path, onCopy, onReveal }: { icon: "database" | "folder-open"; label: string; path: string; onCopy: () => void; onReveal?: () => void }) {
  return <div className="mt-2 grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 text-[12px]">
    <Icon icon={icon} className="text-[var(--text-mute)]" />
    <span className="min-w-0"><span className="block text-[11px] text-[var(--text-mute)]">{label}</span><span className="block truncate font-mono text-[11px] text-[var(--text-dim)]" title={path}>{path}</span></span>
    <span className="flex items-center gap-1"><Button size="icon" variant="ghost" className="size-7" title="Copy full path" aria-label="Copy full path" onClick={onCopy}><Icon icon="clipboard" /></Button>{onReveal && <Button size="icon" variant="ghost" className="size-7" title="Open parent folder" aria-label="Open parent folder" onClick={onReveal}><Icon icon="folder-open" /></Button>}</span>
  </div>;
}

function Notice({ tone, text }: { tone: "success" | "danger"; text: string }) { return <Card><CardContent className="flex items-center gap-3 py-4"><Badge tone={tone}>{tone}</Badge><p className="text-sm text-[var(--text-dim)]">{text}</p></CardContent></Card>; }
function EmptyState() { return <p className="py-6 text-center text-sm text-[var(--text-mute)]">No safe candidates found.</p>; }
