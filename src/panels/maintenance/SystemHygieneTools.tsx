import { useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../../components/ui/alert-dialog";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import { CheckboxControl } from "../../components/ui/bp";
import { Tabs, TabsList, TabsTrigger } from "../../components/ui/tabs";
import type { BrokenShortcut } from "../../hooks/useBackend";
import { useBackend } from "../../hooks/useBackend";
import { formatBytes } from "./routineCleanerHelpers";
import { useSystemHygiene, type SystemHygieneTool } from "./useSystemHygiene";
import { showSuccess } from "../../utils/toast";

export function SystemHygieneTools() {
  const tools = useSystemHygiene();
  const { openPath } = useBackend();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const items =
    tools.tool === "shortcuts"
      ? tools.shortcuts?.shortcuts
      : tools.tool === "environment"
        ? tools.environment?.entries
        : tools.leftovers?.entries;
  const scanLabel = items ? `Rescan ${tools.tool}` : `Scan ${tools.tool}`;

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="flex-row items-start justify-between gap-3">
          <div className="min-w-0 flex-1">
            <CardTitle>System hygiene</CardTitle>
            <CardDescription>
              Audit broken shortcuts, stale environment paths, and conservative
              uninstall leftovers. Nothing is preselected.
            </CardDescription>
          </div>
          <div className="ml-auto flex shrink-0 items-center gap-2">
            {items && (
              <Badge tone="accent">
                {items.length} candidate{items.length === 1 ? "" : "s"}
              </Badge>
            )}
            {tools.busy && tools.tool !== "environment" && (
              <Button
                size="icon"
                variant="outline"
                onClick={() => void tools.cancel()}
                title={`Cancel ${tools.tool} scan`}
                aria-label={`Cancel ${tools.tool} scan`}
              >
                <Icon icon="stop" />
              </Button>
            )}
            <Button
              size="icon"
              variant="primary"
              disabled={tools.busy}
              onClick={() => void tools.scan()}
              title={scanLabel}
              aria-label={scanLabel}
            >
              <Icon
                icon={tools.busy || items ? "refresh" : "search"}
                className={tools.busy ? "animate-spin" : undefined}
              />
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          <Tabs
            value={tools.tool}
            onValueChange={(value) =>
              tools.changeTool(value as SystemHygieneTool)
            }
          >
            <TabsList className="relative isolate overflow-hidden">
              <TabsTrigger value="shortcuts">Shortcuts</TabsTrigger>
              <TabsTrigger value="environment">Environment</TabsTrigger>
              <TabsTrigger value="leftovers">Uninstall leftovers</TabsTrigger>
            </TabsList>
          </Tabs>
        </CardContent>
      </Card>

      {tools.error && <Notice tone="danger" text={tools.error} />}
      {tools.summary && <Notice tone="success" text={tools.summary} />}

      {tools.busy && !items && (
        <Card><CardContent className="py-6 text-center text-sm text-[var(--text-mute)]" role="status" aria-live="polite">
          Reviewing {tools.tool === "shortcuts" ? "broken shortcuts" : tools.tool === "environment" ? "environment entries" : "uninstall leftovers"}…
        </CardContent></Card>
      )}

      {tools.tool === "shortcuts" && tools.shortcuts && (
        <Card>
          <CardHeader>
            <div>
              <CardTitle>Broken shortcuts</CardTitle>
              <CardDescription>
                {tools.shortcuts.scannedShortcuts.toLocaleString()} local
                shortcuts inspected. URL, UWP, namespace, Windows, and
                unresolved targets are excluded.
              </CardDescription>
            </div>
          </CardHeader>
          <CardContent className="grid max-h-[min(30rem,calc(100vh-20rem))] gap-2 overflow-y-auto overscroll-contain pr-1 xl:grid-cols-2">
            {tools.shortcuts.shortcuts.map((item) => (
              <BrokenShortcutRow
                key={item.id}
                checked={tools.selected.has(item.id)}
                onClick={() => tools.select(item.id)}
                shortcut={item}
                onReveal={(path) => void openParentFolder(openPath, path)}
              />
            ))}
            {!tools.shortcuts.shortcuts.length && <Empty />}
          </CardContent>
        </Card>
      )}

      {tools.tool === "environment" && tools.environment && (
        <Card>
          <CardHeader>
            <CardTitle>Stale environment entries</CardTitle>
            <CardDescription>
              PATH and selected directory variables are expanded and checked.
              Current registry values are re-read and backed up before repair.
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-2 xl:grid-cols-2">
            {tools.environment.entries.map((item) => (
              <Row
                key={item.id}
                checked={tools.selected.has(item.id)}
                onClick={() => tools.select(item.id)}
                title={`${item.scope} · ${item.variable}`}
                detail={`${item.kind} · ${item.value}`}
                path={item.value}
                onReveal={(path) => void openParentFolder(openPath, path)}
              />
            ))}
            {!tools.environment.entries.length && <Empty />}
          </CardContent>
        </Card>
      )}

      {tools.tool === "leftovers" && tools.leftovers && (
        <Card>
          <CardHeader>
            <CardTitle>Possible uninstall leftovers</CardTitle>
            <CardDescription>
              {tools.leftovers.scannedFolders.toLocaleString()} app-data folders
              inspected. Recent, installed-app-matching, running, tiny,
              protected, and linked folders are excluded.
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-2 xl:grid-cols-2">
            {tools.leftovers.entries.map((item) => (
              <Row
                key={item.id}
                checked={tools.selected.has(item.id)}
                onClick={() => tools.select(item.id)}
                title={`${item.name} · ${formatBytes(item.bytes)}`}
                detail={`${item.scope} · ${item.path}`}
                path={item.path}
                onReveal={(path) => void openParentFolder(openPath, path)}
              />
            ))}
            {!tools.leftovers.entries.length && <Empty />}
          </CardContent>
        </Card>
      )}

      {!!tools.selected.size && (
        <div className="flex justify-end">
          <Button variant="danger" onClick={() => setConfirmOpen(true)}>
            {tools.tool === "environment" ? "Repair" : "Remove"}{" "}
            {tools.selected.size} selected
          </Button>
        </div>
      )}
      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {tools.tool === "environment"
                ? "Repair environment entries?"
                : "Remove selected items?"}
            </AlertDialogTitle>
            <AlertDialogDescription>
              Every target is revalidated against its preview. Changed or
              out-of-scope targets are refused; environment edits retain
              registry backups.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Back</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                setConfirmOpen(false);
                void tools.apply();
              }}
            >
              Continue
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function BrokenShortcutRow({
  checked,
  onClick,
  shortcut,
  onReveal,
}: {
  checked: boolean;
  onClick: () => void;
  shortcut: BrokenShortcut;
  onReveal: (path: string) => void;
}) {
  return (
    <div className="flex w-full items-start gap-3 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 text-left hover:bg-[var(--surface-2)]">
      <CheckboxControl checked={checked} onChange={onClick} ariaLabel={`${checked ? "Deselect" : "Select"} broken shortcut ${shortcut.name}`} className="mt-0.5" />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5 text-sm text-[var(--text)]">
          <Icon icon="document" size={14} className="shrink-0 text-[var(--text-mute)]" />
          <span className="truncate">{shortcut.name}</span>
        </span>
        <ShortcutPath icon="folder-open" label="Shortcut location" path={shortcut.path} onReveal={onReveal} />
        <ShortcutPath icon="warning-sign" label="Missing target" path={shortcut.target} onReveal={onReveal} />
      </span>
    </div>
  );
}

function ShortcutPath({
  icon,
  label,
  path,
  onReveal,
}: {
  icon: "folder-open" | "warning-sign";
  label: string;
  path: string;
  onReveal: (path: string) => void;
}) {
  const copyPath = async () => {
    const copied = await navigator.clipboard.writeText(path).then(() => true).catch(() => false);
    if (copied) showSuccess("Full location copied.");
  };
  return (
    <span className="mt-1 grid grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-x-1.5">
      <Icon icon={icon} size={12} className="mt-0.5 text-[var(--text-mute)]" />
      <span className="min-w-0">
        <span className="block text-[10px] uppercase tracking-wide text-[var(--text-mute)]">{label}</span>
        <button type="button" onClick={() => onReveal(path)} className="block w-full text-left font-mono text-[11px] text-[var(--text-dim)] line-clamp-2 hover:text-[var(--accent)]" title={`Open containing folder in Explorer: ${path}`}>
          <CompactPath path={path} />
        </button>
      </span>
      <span className="mt-3 flex items-center gap-0.5">
        <Button size="icon" variant="ghost" className="size-6" title="Copy full location" aria-label={`Copy ${label}`} onClick={() => void copyPath()}><Icon icon="clipboard" size={13} /></Button>
        <Button size="icon" variant="ghost" className="size-6" title="Open containing folder" aria-label={`Open ${label} in Explorer`} onClick={() => onReveal(path)}><Icon icon="folder-open" size={13} /></Button>
      </span>
    </span>
  );
}

function Row({
  checked,
  onClick,
  title,
  detail,
  path,
  onReveal,
}: {
  checked: boolean;
  onClick: () => void;
  title: string;
  detail: string;
  path: string;
  onReveal: (path: string) => void;
}) {
  const copyPath = async () => {
    const copied = await navigator.clipboard.writeText(path).then(() => true).catch(() => false);
    if (copied) showSuccess("Full location copied.");
  };
  return (
    <div className="flex w-full items-start gap-3 rounded-[var(--r)] border border-[var(--border)] px-3 py-2 text-left hover:bg-[var(--surface-2)]">
      <CheckboxControl checked={checked} onChange={onClick} ariaLabel={`${checked ? "Deselect" : "Select"} ${title}`} className="mt-0.5" />
      <span className="min-w-0 flex-1">
        <span className="block text-sm text-[var(--text)]">{title}</span>
        <button type="button" onClick={() => onReveal(path)} className="block text-left font-mono text-[11px] text-[var(--text-mute)] hover:text-[var(--accent)]" title={`Open containing folder in Explorer: ${path}`}><CompactPath path={detail} /></button>
      </span>
      <span className="flex shrink-0 items-center gap-0.5">
        <Button size="icon" variant="ghost" className="size-7" title="Copy full location" aria-label={`Copy ${title} location`} onClick={() => void copyPath()}><Icon icon="clipboard" size={14} /></Button>
        <Button size="icon" variant="ghost" className="size-7" title="Open containing folder" aria-label={`Open ${title} in Explorer`} onClick={() => onReveal(path)}><Icon icon="folder-open" size={14} /></Button>
      </span>
    </div>
  );
}

function openParentFolder(openPath: (path: string) => Promise<unknown>, path: string) {
  const normalized = path.replace(/\//g, "\\");
  const separator = normalized.lastIndexOf("\\");
  const folder = separator > 2 ? normalized.slice(0, separator) : normalized;
  return openPath(folder).catch(() => {});
}

function CompactPath({ path }: { path: string }) {
  const normalized = path.replace(/^[A-Za-z]:\\/, (root) => `${root.slice(0, 2)} › `).replace(/\\/g, " › ");
  const isWindows = /^[A-Za-z]:\\Windows(?:\\|$)/i.test(path);
  const isProgramFiles = /^[A-Za-z]:\\Program Files(?: \(x86\))?(?:\\|$)/i.test(path);
  return <span className="inline-flex min-w-0 items-center gap-1"><Icon icon="drive-time" size={11} className="shrink-0 text-[var(--text-mute)]" />{isWindows || isProgramFiles ? <Icon icon={isWindows ? "desktop" : "folder-shared"} size={11} className="shrink-0 text-[var(--text-mute)]" /> : null}<span>{normalized}</span></span>;
}
function Notice({ tone, text }: { tone: "success" | "danger"; text: string }) {
  return (
    <Card role={tone === "danger" ? "alert" : "status"}>
      <CardContent className="flex items-center gap-3 py-4">
        <Badge tone={tone}>{tone}</Badge>
        <p className="text-sm text-[var(--text-dim)]">{text}</p>
      </CardContent>
    </Card>
  );
}
function Empty() {
  return (
    <p className="py-6 text-center text-sm text-[var(--text-mute)]">
      No safe candidates found.
    </p>
  );
}
