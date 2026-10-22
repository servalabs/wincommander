import { useEffect, useMemo, useState } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "../../components/ui/alert-dialog";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../../components/ui/dialog";
import { CheckboxControl } from "../../components/ui/bp";
import { Icon, type IconName } from "../../components/ui/icon";
import { Tabs, TabsList, TabsTrigger } from "../../components/ui/tabs";
import SharedEmptyState from "../../components/shared/EmptyState";
import type { DuplicateRemoveResult, EmptyFolderRemoveResult } from "../../hooks/useBackend";
import { getDuplicateSelectedBytes } from "../../lib/maintenanceSelection";
import { formatBytes } from "./routineCleanerHelpers";
import { useFileHygiene } from "./useFileHygiene";
import { DuplicateGroups } from "./DuplicateGroups";
import { CleanupReviewItems } from "./CleanupReviewItems";
import { DuplicateScanStatus } from "./DuplicateScanStatus";

function describeRemoveResult(result: DuplicateRemoveResult | EmptyFolderRemoveResult) {
  const summary = "bytesRecovered" in result
    ? `Removed ${result.filesRemoved} files (${formatBytes(result.bytesRecovered)} logical data).`
    : `Removed ${result.foldersRemoved} empty folders.`;
  if (result.cancelled) return { tone: "warning" as const, title: "Removal cancelled", message: `${summary} Cancelled before finishing.` };
  if (result.errors.length) return { tone: "warning" as const, title: "Completed with issues", message: `${summary} ${result.errors.length} item${result.errors.length === 1 ? "" : "s"} could not be removed.` };
  return { tone: "success" as const, title: "Removal complete", message: summary };
}

export function FileHygieneTools() {
  const tools = useFileHygiene();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [rootsOpen, setRootsOpen] = useState(false);
  // Notices are dismissible rather than floating indefinitely: dismissal
  // resets whenever the underlying error/result identity changes (a fresh
  // scan error or a fresh remove result should reappear even if the last
  // one was dismissed).
  const [errorDismissed, setErrorDismissed] = useState(false);
  const [resultDismissed, setResultDismissed] = useState(false);
  useEffect(() => setErrorDismissed(false), [tools.error]);
  useEffect(() => setResultDismissed(false), [tools.result]);

  const count = tools.tool === "duplicates"
    ? tools.duplicateScan?.totalGroups ?? 0
    : tools.emptyScan?.folders.length ?? 0;
  const hasScan = tools.tool === "duplicates" ? tools.duplicateScan !== undefined : tools.emptyScan !== undefined;
  const scanning = tools.busy && !hasScan;
  const scanLabel = hasScan ? `Rescan ${tools.tool}` : `Scan ${tools.tool}`;
  const resultNotice = tools.result ? describeRemoveResult(tools.result) : null;
  const scannedCount = tools.tool === "duplicates" ? tools.duplicateScan?.scannedFiles ?? 0 : tools.emptyScan?.scannedFolders ?? 0;
  const truncated = tools.tool === "duplicates" ? tools.duplicateScan?.truncated : tools.emptyScan?.truncated;
  const cancelled = tools.tool === "duplicates" ? tools.duplicateScan?.cancelled : tools.emptyScan?.cancelled;
  const selectedBytes = useMemo(() => {
    if (tools.tool !== "duplicates" || !tools.duplicateScan) return 0;
    return getDuplicateSelectedBytes(tools.duplicateScan.groups, tools.selected);
  }, [tools.tool, tools.duplicateScan, tools.selected]);
  // Aggregate removable bytes across every group, not just the selected
  // ones — this is the "total scan findings" figure surfaced immediately
  // after a scan completes, before the user selects anything.
  const totalReclaimableBytes = useMemo(() => {
    if (tools.tool !== "duplicates" || !tools.duplicateScan) return 0;
    return tools.duplicateScan.totalDuplicateBytes;
  }, [tools.tool, tools.duplicateScan]);
  const selectedReview = tools.tool === "duplicates"
    ? tools.duplicateScan?.groups.flatMap((group) => group.files.filter((file) => tools.selected.has(file.id)).map((file) => ({ id: file.id, path: file.path, detail: formatBytes(file.size) }))) ?? []
    : tools.emptyScan?.folders.filter((folder) => tools.selected.has(folder.id)).map((folder) => ({ id: folder.id, path: folder.path, detail: "Empty folder" })) ?? [];

  return (
    <div className="maintenance-file-hygiene-tools flex flex-col gap-4">
      <Card className="maintenance-file-hygiene-card">
        {tools.error && !errorDismissed && <Notice tone="danger" title="Operation failed" message={tools.error} onDismiss={() => setErrorDismissed(true)} />}
        {resultNotice && !resultDismissed && <Notice tone={resultNotice.tone} title={resultNotice.title} message={resultNotice.message} onDismiss={() => setResultDismissed(true)} />}
        <div className="maintenance-file-hygiene-results">
        <CardHeader>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <Icon icon="clean" size={16} className="text-[var(--accent)]" />
              <CardTitle>{tools.tool === "duplicates" ? "Duplicate Cleanup" : "Inspect empty folders"}</CardTitle>
            </div>
            <div className="ml-auto flex flex-wrap items-center justify-end gap-2">
              {!!tools.roots.length && <Badge tone="accent">{tools.roots.length} folder{tools.roots.length === 1 ? "" : "s"}</Badge>}
              {tools.roots.length > 0 && <Button size="sm" variant="outline" onClick={() => setRootsOpen(true)}><Icon icon="folder-open" /> View selected</Button>}
              <Button size="sm" variant="outline" onClick={() => void tools.chooseRoots()} disabled={tools.busy}><Icon icon="folder-open" /> Choose folders</Button>
              {tools.tool === "duplicates" && <Button size="sm" variant="outline" disabled={tools.busy} onClick={() => void tools.scan("local")}><Icon icon="hard-drive" /> Scan local drives</Button>}
              {tools.busy && <Button size="icon" variant="outline" onClick={() => void tools.cancel()} title={`Cancel ${tools.tool} scan`} aria-label={`Cancel ${tools.tool} scan`}><Icon icon="stop" /></Button>}
            </div>
          </div>
          <CardDescription>
            {tools.tool === "duplicates"
              ? "Select copies to remove; one verified copy per group is always retained."
              : "Only folders still empty at removal time are deleted."}
          </CardDescription>
          <Tabs value={tools.tool} onValueChange={(value) => tools.changeTool(value as "duplicates" | "empty")}>
            <TabsList>
              <TabsTrigger value="duplicates">Duplicate files</TabsTrigger>
              <TabsTrigger value="empty">Empty folders</TabsTrigger>
            </TabsList>
          </Tabs>
          {hasScan && (
            <div className="flex flex-wrap items-center gap-2 pt-1">
              {tools.tool === "duplicates" ? (
                <>
                  <Badge tone="accent">{formatBytes(totalReclaimableBytes)} potential duplicate data</Badge>
                  <Badge tone="neutral">{count} group{count === 1 ? "" : "s"}</Badge>
                </>
              ) : (
                <Badge tone="accent">{count} empty folder{count === 1 ? "" : "s"}</Badge>
              )}
              <Badge tone="neutral">{scannedCount.toLocaleString()} {tools.tool === "duplicates" ? "files" : "folders"} inspected</Badge>
              {truncated && <Badge tone="warning">Partial results · scan limit reached</Badge>}
              {cancelled && <Badge tone="warning">cancelled</Badge>}
              {tools.tool === "duplicates" && !!tools.duplicateScan?.skippedEntries && <Badge tone="warning">Partial scope · {tools.duplicateScan.skippedEntries.toLocaleString()} skipped entries</Badge>}
              <Button size="sm" variant="outline" className="ml-auto" onClick={() => void tools.scan()} disabled={tools.busy} title={scanLabel} aria-label={scanLabel}>
                <Icon icon="refresh" className={tools.busy ? "animate-spin" : undefined} /> Rescan
              </Button>
            </div>
          )}
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {tools.tool === "duplicates" && <DuplicateScanStatus progress={tools.progress} scan={tools.duplicateScan} scanning={scanning} local={tools.scopeMode === "local"} />}
          {hasScan && <div className="flex flex-wrap items-center gap-2">
            {tools.tool === "duplicates" && <label className="text-xs text-[var(--text-dim)]">Keep <select aria-label="Duplicate copy retention preference" className="rounded border border-[var(--border)] bg-[var(--surface-2)] p-1.5" value={tools.keepPolicy} disabled={tools.busy} onChange={(event) => tools.changeKeepPolicy(event.target.value as "first" | "shortest-path")}><option value="first">First verified copy</option><option value="shortest-path">Shortest path on this page</option></select></label>}
            <Button size="sm" variant="outline" disabled={tools.busy || !count} onClick={tools.smartSelect}>{tools.tool === "duplicates" ? "Select duplicate copies on this page" : "Select all empty folders"}</Button>
            <Button size="sm" variant="ghost" disabled={tools.busy || !tools.selected.size} onClick={tools.clearSelection}>Clear selection</Button>
          </div>}
          {tools.tool === "duplicates" && hasScan && <p className="text-xs text-[var(--text-dim)]">Contents are verified with SHA-256. The full eligible scope is indexed without a file-count cap; results are paged at 100 groups and 200 additional copies per group. Page changes clear selections. Potential duplicate data is logical size, not measured free space.</p>}
          {/* Exactly one scan entry point per state: none of these four
              branches can render at the same time as another. */}
          {!tools.roots.length && (tools.tool === "empty" || tools.scopeMode === "folders") && (
            <SharedEmptyState
              icon="folder-open"
              title="Choose folders to begin"
              hint="Add one or more folders above, then scan for duplicates or empty folders."
            />
          )}
          {(tools.roots.length > 0 || (tools.tool === "duplicates" && tools.scopeMode === "local")) && !hasScan && !scanning && (
            <SharedEmptyState
              icon="search"
              title={tools.tool === "duplicates" ? "Ready to scan for duplicates" : "Ready to scan for empty folders"}
              hint="Run a scan to preview candidates before anything is removed."
              action={<Button size="sm" variant="primary" onClick={() => void tools.scan()}><Icon icon="search" /> {scanLabel}</Button>}
            />
          )}
          {scanning && (
            <div className="flex items-center gap-3 py-4 text-sm text-[var(--text-dim)]">
              <Icon icon="search-around" className="animate-spin text-[var(--accent)]" />
              Scanning for {tools.tool === "duplicates" ? "duplicate files" : "empty folders"}. You can cancel while the scan completes.
            </div>
          )}

          {tools.tool === "duplicates" && tools.duplicateScan && (
            <>
              <DuplicateGroups tools={tools} />
              <nav aria-label="Duplicate result pages" className="flex flex-wrap items-center justify-between gap-2 text-xs text-[var(--text-dim)]"><span>Groups {count ? tools.duplicateScan.groupOffset + 1 : 0}–{tools.duplicateScan.groupOffset + tools.duplicateScan.groups.length} of {count.toLocaleString()}</span><div className="flex gap-2"><Button size="sm" variant="outline" disabled={tools.busy || tools.duplicateScan.groupOffset === 0} onClick={() => void tools.loadGroups(Math.max(0, tools.duplicateScan!.groupOffset - 100))}>Previous groups</Button><Button size="sm" variant="outline" disabled={tools.busy || !tools.duplicateScan.hasMoreGroups} onClick={() => void tools.loadGroups(tools.duplicateScan!.groupOffset + 100)}>Next groups</Button></div></nav>
              <SelectionActionBar disabled={tools.busy} count={tools.selected.size} bytesLabel={selectedBytes > 0 ? `${formatBytes(selectedBytes)} logical bytes selected` : undefined} onRemove={() => setConfirmOpen(true)} />
            </>
          )}

          {tools.tool === "empty" && tools.emptyScan && (
            <>
              <div className="flex flex-col gap-2">
                {tools.emptyScan.folders.map((folder) => <SelectionRow key={folder.id} checked={tools.selected.has(folder.id)} disabled={tools.busy} onClick={() => tools.select(folder.id)} icon="folder-close" label={folder.name} detail={folder.path} />)}
                {!count && <SharedEmptyState icon="confirm" title="No empty folders found" hint="No empty folders were returned within the completed scan scope." />}
              </div>
              <SelectionActionBar disabled={tools.busy} count={tools.selected.size} onRemove={() => setConfirmOpen(true)} />
            </>
          )}
        </CardContent>
      </div>
      </Card>

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>Permanently remove {selectedReview.length} selected items?</AlertDialogTitle><AlertDialogDescription>{tools.tool === "duplicates" ? `${formatBytes(selectedBytes)} of logical duplicate data selected. At least one unchanged, content-verified copy must remain per group. ` : "Only folders still empty are removed. "}The backend revalidates every selection. This cannot be undone.</AlertDialogDescription></AlertDialogHeader>
          <CleanupReviewItems items={selectedReview} />
          <AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={() => { setConfirmOpen(false); void tools.remove(); }}>Remove selected</AlertDialogAction></AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <Dialog open={rootsOpen} onOpenChange={setRootsOpen}>
        <DialogContent className="max-h-[min(34rem,calc(100vh-2rem))] overflow-hidden">
          <DialogHeader>
            <DialogTitle>Selected folders</DialogTitle>
            <DialogDescription>These folders are included in duplicate-file and empty-folder inspections.</DialogDescription>
          </DialogHeader>
          <div className="mt-4 max-h-[22rem] overflow-y-auto overscroll-contain pr-1">
            <div className="flex flex-col gap-2">
              {tools.roots.map((root) => (
                <div key={root} className="flex min-w-0 items-center gap-2 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-2">
                  <Icon icon="folder-close" size={14} className="shrink-0 text-[var(--text-mute)]" />
                  <span className="min-w-0 flex-1 break-all font-mono text-xs text-[var(--text-dim)]" title={root}>{root}</span>
                  <Button size="icon" variant="ghost" disabled={tools.busy} onClick={() => tools.removeRoot(root)} title={`Remove ${root}`} aria-label={`Remove ${root}`}>
                    <Icon icon="cross" size={14} />
                  </Button>
                </div>
              ))}
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}

// Keeps the destructive action visible after a long page-level results list.
function SelectionActionBar({ count, bytesLabel, onRemove, disabled }: { count: number; bytesLabel?: string; onRemove: () => void; disabled: boolean }) {
  if (!count) return null;
  return (
    <div className="sticky bottom-0 z-[var(--z-sticky)] flex flex-wrap items-center justify-end gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-2.5 shadow-[var(--shadow)]">
      {bytesLabel && <span className="text-xs text-[var(--text-mute)]">{bytesLabel}</span>}
      <Button variant="danger" disabled={disabled} onClick={onRemove}>Remove {count} selected</Button>
    </div>
  );
}

function SelectionRow({ checked, disabled = false, onClick, icon, label, detail }: { checked: boolean; disabled?: boolean; onClick: () => void; icon?: IconName; label: string; detail: string }) {
  return (
    <div className={`flex w-full items-start gap-3 rounded-[var(--r)] px-3 py-2 text-left transition-colors hover:bg-[var(--surface-2)] ${disabled ? "cursor-default opacity-70" : "cursor-pointer"}`}>
      <CheckboxControl checked={checked} disabled={disabled} ariaLabel={`${checked ? "Deselect" : "Select"} ${label}`} onChange={onClick} />
      {icon && <Icon icon={icon} size={14} className="mt-0.5 shrink-0 text-[var(--text-mute)]" />}
      <span className="min-w-0">
        <span className="flex items-center gap-2">
          <span className="block text-sm text-[var(--text)]">{label}</span>
        </span>
        <span className="block break-all font-mono text-[11px] text-[var(--text-mute)]">{detail}</span>
      </span>
    </div>
  );
}

function Notice({ tone, title, message, onDismiss }: { tone: "success" | "warning" | "danger"; title: string; message: string; onDismiss: () => void }) {
  return (
    <Card>
      <CardContent className="flex items-start gap-3 py-4">
        <Badge tone={tone}>{title}</Badge>
        <p className="min-w-0 flex-1 text-sm text-[var(--text-dim)]">{message}</p>
        <button type="button" onClick={onDismiss} aria-label="Dismiss notice" title="Dismiss" className="shrink-0 rounded-[var(--r-sm)] p-1 text-[var(--text-mute)] hover:bg-[var(--surface-3)] hover:text-[var(--text)]">
          <Icon icon="cross" size={14} />
        </button>
      </CardContent>
    </Card>
  );
}
