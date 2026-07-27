import { useState } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "../../components/ui/alert-dialog";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import { Tabs, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { useRegistryTools } from "./useRegistryTools";

type Action = "disable" | "enable" | "remove";

export function RegistryTools() {
  const tools = useRegistryTools();
  const [pendingAction, setPendingAction] = useState<Action>();
  const entries = tools.tool === "orphans" ? tools.registryScan?.entries : tools.contextScan?.entries;
  const scanLabel = entries ? `Rescan ${tools.tool}` : `Scan ${tools.tool}`;

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Registry and Explorer hygiene</CardTitle>
          <CardDescription>Conservative scans cover orphaned per-user COM servers and explicit third-party Explorer verbs. Windows and WinCommander entries are protected.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <Tabs value={tools.tool} onValueChange={(value) => tools.changeTool(value as "orphans" | "context")}>
            <TabsList><TabsTrigger value="orphans">Registry orphans</TabsTrigger><TabsTrigger value="context">Context menu</TabsTrigger></TabsList>
          </Tabs>
          <div className="flex items-center gap-2">
            {entries && <Badge tone="accent">{entries.length} candidate{entries.length === 1 ? "" : "s"}</Badge>}
            <Button size="sm" variant="primary" className="ml-auto" disabled={tools.busy} onClick={() => void tools.scan()} title={scanLabel} aria-label={scanLabel}><Icon icon={tools.busy || entries ? "refresh" : "search"} className={tools.busy ? "animate-spin" : undefined} />{scanLabel}</Button>
          </div>
        </CardContent>
      </Card>

      {tools.error && <Notice tone="danger" text={tools.error} />}
      {tools.result && <Notice tone="success" text={`Changed ${"removed" in tools.result ? tools.result.removed : tools.result.changed} entries. ${tools.result.backupLocations.length} registry backups retained.`} />}

      {tools.tool === "orphans" && tools.registryScan && (
        <Card>
          <CardHeader><CardTitle>Orphaned per-user COM registrations</CardTitle><CardDescription>Only syntactically valid HKCU CLSID registrations with a now-missing absolute server executable are listed.</CardDescription></CardHeader>
          <CardContent className="flex flex-col gap-2">
            {tools.registryScan.entries.map((entry) => <EntryRow key={entry.id} checked={tools.selected.has(entry.id)} onClick={() => tools.select(entry.id)} title={`${entry.hive} · ${entry.classId}`} detail={`${entry.serverKind} → ${entry.missingServer}`} enabled />)}
            {!tools.registryScan.entries.length && <EmptyState />}
          </CardContent>
        </Card>
      )}

      {tools.tool === "context" && tools.contextScan && (
        <Card>
          <CardHeader><CardTitle>Third-party Explorer verbs</CardTitle><CardDescription>Disabled entries remain recoverable; removal keeps an app-owned registry backup. Hover a row for its full command.</CardDescription></CardHeader>
          <CardContent className="flex flex-col gap-1">
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

function EntryRow({ checked, onClick, title, detail, enabled }: { checked: boolean; onClick: () => void; title: string; detail: string; enabled: boolean }) {
  return <button type="button" onClick={onClick} className="flex w-full items-start gap-3 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 text-left hover:bg-[var(--surface-2)]"><span className={`mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-[var(--r-sm)] border ${checked ? "border-[var(--accent)] bg-[var(--accent)] text-[var(--accent-contrast)]" : "border-[var(--border-strong)]"}`}>{checked && <Icon icon="check" />}</span><span className="min-w-0 flex-1"><span className="block text-sm text-[var(--text)]">{title}</span><span className="block break-all font-mono text-[11px] text-[var(--text-mute)]">{detail}</span></span><Badge tone={enabled ? "success" : "warning"}>{enabled ? "enabled" : "disabled"}</Badge></button>;
}

// One compact row per verb instead of a wrapping name+location+command block --
// the full command (which can be long) sits behind the row's title tooltip.
function ContextVerbRow({ checked, onClick, label, location, command, enabled }: { checked: boolean; onClick: () => void; label: string; location: string; command: string; enabled: boolean }) {
  return <button type="button" onClick={onClick} title={command} className="grid w-full grid-cols-[auto_1fr_1fr_auto] items-center gap-3 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 text-left hover:bg-[var(--surface-2)]"><span className={`flex size-4 shrink-0 items-center justify-center rounded-[var(--r-sm)] border ${checked ? "border-[var(--accent)] bg-[var(--accent)] text-[var(--accent-contrast)]" : "border-[var(--border-strong)]"}`}>{checked && <Icon icon="check" />}</span><span className="min-w-0 truncate text-sm text-[var(--text)]">{label}</span><span className="min-w-0 truncate font-mono text-[11px] text-[var(--text-mute)]">{location}</span><Badge tone={enabled ? "success" : "warning"}>{enabled ? "enabled" : "disabled"}</Badge></button>;
}

function Notice({ tone, text }: { tone: "success" | "danger"; text: string }) { return <Card><CardContent className="flex items-center gap-3 py-4"><Badge tone={tone}>{tone}</Badge><p className="text-sm text-[var(--text-dim)]">{text}</p></CardContent></Card>; }
function EmptyState() { return <p className="py-6 text-center text-sm text-[var(--text-mute)]">No safe candidates found.</p>; }
