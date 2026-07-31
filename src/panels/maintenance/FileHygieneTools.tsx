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
import { formatBytes } from "./routineCleanerHelpers";
import { useFileHygiene } from "./useFileHygiene";

// Extension → icon glyph, mirrors the file-type iconography the Disk Space
// Analyzer uses so duplicate rows read consistently with the rest of Maintenance.
const EXT_ICONS: Record<string, IconName> = {
  jpg: "media", jpeg: "media", png: "media", gif: "media", webp: "media", bmp: "media", svg: "media", heic: "media",
  mp4: "video", mkv: "video", mov: "video", avi: "video", wmv: "video", webm: "video",
  mp3: "music", wav: "music", flac: "music", m4a: "music", ogg: "music", aac: "music",
  zip: "compressed", rar: "compressed", "7z": "compressed", tar: "compressed", gz: "compressed", iso: "compressed",
  pdf: "document", doc: "document", docx: "document", txt: "document", md: "document", rtf: "document",
  xls: "th", xlsx: "th", csv: "th",
  exe: "application", msi: "application", appx: "application",
  js: "code", ts: "code", tsx: "code", jsx: "code", py: "code", rs: "code", html: "code", css: "code", json: "code",
};
function fileIconFor(name: string): IconName {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  return EXT_ICONS[ext] ?? "document";
}

function describeRemoveResult(result: DuplicateRemoveResult | EmptyFolderRemoveResult) {
  const summary = "bytesRecovered" in result
    ? `Removed ${result.filesRemoved} files and recovered ${formatBytes(result.bytesRecovered)}.`
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
    ? tools.duplicateScan?.groups.length ?? 0
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
    const files = tools.duplicateScan.groups.flatMap((group) => group.files);
    return files.filter((file) => tools.selected.has(file.id)).reduce((sum, file) => sum + file.size, 0);
  }, [tools.tool, tools.duplicateScan, tools.selected]);
  // Aggregate reclaimable bytes across every group, not just the selected
  // ones — this is the "total scan findings" figure surfaced immediately
  // after a scan completes, before the user selects anything.
  const totalReclaimableBytes = useMemo(() => {
    if (tools.tool !== "duplicates" || !tools.duplicateScan) return 0;
    return tools.duplicateScan.groups.reduce((sum, group) => sum + group.reclaimableBytes, 0);
  }, [tools.tool, tools.duplicateScan]);

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
              <CardTitle>Inspect selected folders</CardTitle>
            </div>
            <div className="ml-auto flex flex-wrap items-center justify-end gap-2">
              {!!tools.roots.length && <Badge tone="accent">{tools.roots.length} folder{tools.roots.length === 1 ? "" : "s"}</Badge>}
              {tools.roots.length > 0 && <Button size="sm" variant="outline" onClick={() => setRootsOpen(true)}><Icon icon="folder-open" /> View selected</Button>}
              <Button size="sm" variant="outline" onClick={() => void tools.chooseRoots()} disabled={tools.busy}><Icon icon="folder-open" /> Choose folders</Button>
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
                  <Badge tone="accent">{formatBytes(totalReclaimableBytes)} reclaimable</Badge>
                  <Badge tone="neutral">{count} group{count === 1 ? "" : "s"}</Badge>
                </>
              ) : (
                <Badge tone="accent">{count} empty folder{count === 1 ? "" : "s"}</Badge>
              )}
              <Badge tone="neutral">{scannedCount.toLocaleString()} {tools.tool === "duplicates" ? "files" : "folders"} inspected</Badge>
              {truncated && <Badge tone="warning">truncated</Badge>}
              {cancelled && <Badge tone="warning">cancelled</Badge>}
              <Button size="sm" variant="outline" className="ml-auto" onClick={() => void tools.scan()} disabled={tools.busy} title={scanLabel} aria-label={scanLabel}>
                <Icon icon="refresh" className={tools.busy ? "animate-spin" : undefined} /> Rescan
              </Button>
            </div>
          )}
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          {/* Exactly one scan entry point per state: none of these four
              branches can render at the same time as another. */}
          {!tools.roots.length && (
            <SharedEmptyState
              icon="folder-open"
              title="Choose folders to begin"
              hint="Add one or more folders above, then scan for duplicates or empty folders."
            />
          )}
          {!!tools.roots.length && !hasScan && !scanning && (
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
              <div className="flex flex-col gap-3">
                {tools.duplicateScan.groups.map((group) => (
                  <div key={group.id} className="rounded-[var(--r)] border border-[var(--border)] p-3 transition-colors hover:border-[var(--border-strong)]">
                    <div className="mb-2 flex items-center justify-between gap-2">
                      <span className="flex items-center gap-1.5 font-mono text-xs text-[var(--text-dim)]"><Icon icon="duplicate" size={12} />{group.files.length} identical files</span>
                      <Badge tone="warning">{formatBytes(group.reclaimableBytes)} reclaimable</Badge>
                    </div>
                    <div className="flex flex-col gap-0.5">
                      {group.files.map((file, index) => (
                        <SelectionRow key={file.id} checked={tools.selected.has(file.id)} disabled={index === 0} onClick={() => tools.select(file.id)} icon={fileIconFor(file.name)} label={file.name} detail={file.path} retained={index === 0} />
                      ))}
                    </div>
                  </div>
                ))}
                {!count && <SharedEmptyState icon="confirm" title="No duplicate file groups found" hint="Every file across the chosen folders is unique." />}
              </div>
              <SelectionActionBar count={tools.selected.size} bytesLabel={selectedBytes > 0 ? `${formatBytes(selectedBytes)} to reclaim` : undefined} onRemove={() => setConfirmOpen(true)} />
            </>
          )}

          {tools.tool === "empty" && tools.emptyScan && (
            <>
              <div className="flex flex-col gap-2">
                {tools.emptyScan.folders.map((folder) => <SelectionRow key={folder.id} checked={tools.selected.has(folder.id)} onClick={() => tools.select(folder.id)} icon="folder-close" label={folder.name} detail={folder.path} />)}
                {!count && <SharedEmptyState icon="confirm" title="No empty folders found" hint="Every folder across the chosen folders is in use." />}
              </div>
              <SelectionActionBar count={tools.selected.size} onRemove={() => setConfirmOpen(true)} />
            </>
          )}
        </CardContent>
      </div>
      </Card>

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader><AlertDialogTitle>Remove selected items?</AlertDialogTitle><AlertDialogDescription>The backend will revalidate every selection and refuse changed, linked, protected, or out-of-root targets.</AlertDialogDescription></AlertDialogHeader>
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
                  <Button size="icon" variant="ghost" onClick={() => tools.removeRoot(root)} title={`Remove ${root}`} aria-label={`Remove ${root}`}>
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
function SelectionActionBar({ count, bytesLabel, onRemove }: { count: number; bytesLabel?: string; onRemove: () => void }) {
  if (!count) return null;
  return (
    <div className="sticky bottom-0 z-[var(--z-sticky)] flex flex-wrap items-center justify-end gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-2.5 shadow-[var(--shadow)]">
      {bytesLabel && <span className="text-xs text-[var(--text-mute)]">{bytesLabel}</span>}
      <Button variant="danger" onClick={onRemove}>Remove {count} selected</Button>
    </div>
  );
}

function SelectionRow({ checked, disabled = false, onClick, icon, label, detail, retained = false }: { checked: boolean; disabled?: boolean; onClick: () => void; icon?: IconName; label: string; detail: string; retained?: boolean }) {
  return (
    <div onClick={() => !disabled && onClick()} className={`flex w-full items-start gap-3 rounded-[var(--r)] px-3 py-2 text-left transition-colors hover:bg-[var(--surface-2)] ${disabled ? "cursor-default opacity-70" : "cursor-pointer"}`}>
      <CheckboxControl checked={checked} disabled={disabled} ariaLabel={`Select ${label}`} onChange={onClick} onClick={(event) => event.stopPropagation()} />
      {icon && <Icon icon={icon} size={14} className="mt-0.5 shrink-0 text-[var(--text-mute)]" />}
      <span className="min-w-0">
        <span className="flex items-center gap-2">
          <span className="block text-sm text-[var(--text)]">{label}</span>
          {retained && <Badge tone="neutral">retained</Badge>}
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
