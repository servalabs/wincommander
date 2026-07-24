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
import { formatBytes } from "./routineCleanerHelpers";
import { useFileHygiene } from "./useFileHygiene";

export function FileHygieneTools() {
  const tools = useFileHygiene();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const count = tools.tool === "duplicates"
    ? tools.duplicateScan?.groups.length ?? 0
    : tools.emptyScan?.folders.length ?? 0;
  const hasScan = tools.tool === "duplicates" ? tools.duplicateScan !== undefined : tools.emptyScan !== undefined;
  const scanLabel = hasScan ? `Rescan ${tools.tool}` : `Scan ${tools.tool}`;

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>File hygiene</CardTitle>
          <CardDescription>Choose explicit folders. WinCommander never accepts paths during removal—only short-lived IDs from the preview.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <Tabs value={tools.tool} onValueChange={(value) => tools.changeTool(value as "duplicates" | "empty")}>
            <TabsList>
              <TabsTrigger value="duplicates">Duplicate files</TabsTrigger>
              <TabsTrigger value="empty">Empty folders</TabsTrigger>
            </TabsList>
          </Tabs>
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="outline" onClick={() => void tools.chooseRoots()} disabled={tools.busy}>
              <Icon icon="folder-open" /> Choose folders
            </Button>
            <Button size="icon" variant="primary" onClick={() => void tools.scan()} disabled={tools.busy || !tools.roots.length} title={scanLabel} aria-label={scanLabel}>
              <Icon icon={tools.busy || hasScan ? "refresh" : "search"} className={tools.busy ? "animate-spin" : undefined} />
            </Button>
            {tools.busy && <Button size="icon" variant="outline" onClick={() => void tools.cancel()} title={`Cancel ${tools.tool} scan`} aria-label={`Cancel ${tools.tool} scan`}><Icon icon="stop" /></Button>}
            {!!tools.roots.length && <Badge tone="accent">{tools.roots.length} root{tools.roots.length === 1 ? "" : "s"}</Badge>}
          </div>
          {!!tools.roots.length && <p className="break-all font-mono text-xs text-[var(--text-mute)]">{tools.roots.join(" · ")}</p>}
        </CardContent>
      </Card>

      {tools.error && <Notice tone="danger">{tools.error}</Notice>}
      {tools.result && <Notice tone={tools.result.errors.length ? "warning" : "success"}>
        {"bytesRecovered" in tools.result
          ? `Removed ${tools.result.filesRemoved} files and recovered ${formatBytes(tools.result.bytesRecovered)}.`
          : `Removed ${tools.result.foldersRemoved} empty folders.`}
      </Notice>}

      {tools.tool === "duplicates" && tools.duplicateScan && (
        <Card>
          <CardHeader>
            <CardTitle>{count} duplicate groups</CardTitle>
            <CardDescription>{tools.duplicateScan.scannedFiles.toLocaleString()} files inspected. Select copies to remove; one verified copy per group is always retained.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            {tools.duplicateScan.groups.map((group) => (
              <div key={group.id} className="rounded-[var(--r)] border border-[var(--border)] p-3">
                <div className="mb-2 flex items-center justify-between gap-2 text-xs">
                  <span className="font-mono text-[var(--text-dim)]">{group.files.length} identical files</span>
                  <Badge tone="warning">{formatBytes(group.reclaimableBytes)} reclaimable</Badge>
                </div>
                {group.files.map((file, index) => (
                  <SelectionRow key={file.id} checked={tools.selected.has(file.id)} disabled={index === 0} onClick={() => tools.select(file.id)} label={file.name} detail={index === 0 ? `${file.path} · retained` : file.path} />
                ))}
              </div>
            ))}
            {!count && <EmptyState text="No duplicate file groups found." />}
          </CardContent>
        </Card>
      )}

      {tools.tool === "empty" && tools.emptyScan && (
        <Card>
          <CardHeader>
            <CardTitle>{count} empty folders</CardTitle>
            <CardDescription>{tools.emptyScan.scannedFolders.toLocaleString()} folders inspected. Only folders still empty at removal time are deleted.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-2">
            {tools.emptyScan.folders.map((folder) => <SelectionRow key={folder.id} checked={tools.selected.has(folder.id)} onClick={() => tools.select(folder.id)} label={folder.name} detail={folder.path} />)}
            {!count && <EmptyState text="No empty folders found." />}
          </CardContent>
        </Card>
      )}

      {!!tools.selected.size && <div className="flex justify-end"><Button variant="danger" onClick={() => setConfirmOpen(true)}>Remove {tools.selected.size} selected</Button></div>}
      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>Remove selected items?</AlertDialogTitle><AlertDialogDescription>The backend will revalidate every selection and refuse changed, linked, protected, or out-of-root targets.</AlertDialogDescription></AlertDialogHeader>
          <AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={() => { setConfirmOpen(false); void tools.remove(); }}>Remove selected</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function SelectionRow({ checked, disabled = false, onClick, label, detail }: { checked: boolean; disabled?: boolean; onClick: () => void; label: string; detail: string }) {
  return <button type="button" disabled={disabled} onClick={onClick} className="flex w-full items-start gap-3 rounded-[var(--r)] px-3 py-2 text-left hover:bg-[var(--surface-2)] disabled:opacity-70"><span className={`mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-[var(--r-sm)] border ${checked ? "border-[var(--accent)] bg-[var(--accent)] text-[var(--accent-contrast)]" : "border-[var(--border-strong)]"}`}>{checked && <Icon icon="check" />}</span><span className="min-w-0"><span className="block text-sm text-[var(--text)]">{label}</span><span className="block break-all font-mono text-[11px] text-[var(--text-mute)]">{detail}</span></span></button>;
}

function Notice({ tone, children }: { tone: "success" | "warning" | "danger"; children: string }) { return <Card><CardContent className="flex items-center gap-3 py-4"><Badge tone={tone}>{tone}</Badge><p className="text-sm text-[var(--text-dim)]">{children}</p></CardContent></Card>; }
function EmptyState({ text }: { text: string }) { return <p className="py-6 text-center text-sm text-[var(--text-mute)]">{text}</p>; }
