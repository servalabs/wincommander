import { useEffect, useState } from "react";
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
import { Card, CardContent } from "../../components/ui/card";
import { Icon } from "../../components/ui/icon";
import type { RoutineCleanerCategory, RoutineCleanerCleanResult } from "../../hooks/useBackend";
import { showError, showSuccess } from "../../utils/toast";
import { formatBytes, getRoutineCleanerCategories } from "./routineCleanerHelpers";
import { RoutineCleanerPreview } from "./RoutineCleanerPreview";
import { CleanupReviewItems } from "./CleanupReviewItems";
import { toExplorerDisplayPath } from "./maintenancePathDisplay";
import { type useRoutineCleaner } from "./useRoutineCleaner";

type RoutineCleaner = ReturnType<typeof useRoutineCleaner>;

/**
 * Preview-and-confirm cache cleaner. Renders bare (no card chrome of its own)
 * because it is one scope inside the "Reclaim disk space" card — the caller
 * owns the heading and the categories.
 */
export function RoutineCleanerPanel({ cleaner, categories: allowedCategories }: { cleaner: RoutineCleaner; categories: RoutineCleanerCategory[] }) {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const { categories, operation, scan, selectedItems, result, error } = cleaner;
  const isRunning = operation !== "idle";
  const categoryOptions = getRoutineCleanerCategories(allowedCategories);
  const scanLabel = scan && cleaner.scanScope !== "projects" ? "Rescan application caches" : "Scan application caches";
  const selectedLogicalBytes = selectedItems.reduce((total, item) => total + (item.operation === "delete" ? item.bytes : 0), 0);
  const selectedVacuumEstimate = selectedItems.reduce((total, item) => total + (item.operation === "vacuum" ? item.bytes : 0), 0);
  const reviewEntries = selectedItems.flatMap((item) => {
    const entries = item.entries ?? [];
    const entryCount = item.entryCount ?? item.fileCount;
    const amount = item.operation === "vacuum"
      ? `${formatBytes(item.bytes)} estimated reclaim`
      : `${formatBytes(item.bytes)} logical file size`;
    const limitNotice = item.entriesTruncated
      ? ` · showing ${entries.length} of ${entryCount} file paths`
      : "";
    const partialNotice = item.truncated ? " · scan was partial" : "";
    return [
      {
        id: `target:${item.id}`,
        path: toExplorerDisplayPath(item.pathDisplay) ?? "Path unavailable",
        detail: `${item.label} · ${entryCount.toLocaleString()} files counted · ${amount}${limitNotice}${partialNotice}`,
      },
      ...entries.map((entry) => ({
        id: `entry:${item.id}:${entry.id}`,
        path: toExplorerDisplayPath(entry.path) ?? "Path unavailable",
        detail: `${item.label} · ${formatBytes(entry.actualSizeBytes)} file size${entry.estimatedReclaimBytes === null ? "" : ` · ${formatBytes(entry.estimatedReclaimBytes)} estimated reclaim`}`,
      })),
    ];
  });

  useEffect(() => {
    if (error) void showError(error);
  }, [error]);

  useEffect(() => {
    if (result && !result.cancelled && !result.errors.length) {
      void showSuccess(`Removed ${formatBytes(result.bytesRecovered)} of logical data from ${result.filesCleaned.toLocaleString()} files.`);
    }
  }, [result]);

  const runClean = async () => {
    setConfirmOpen(false);
    await cleaner.cleanSelected();
  };

  return <div className="maintenance-routine-cleaner flex min-h-0 flex-1 flex-col gap-4">
    <div className="flex flex-wrap items-start gap-2">
      <div className="inline-flex flex-wrap items-center gap-1 rounded-[var(--r)] bg-[var(--surface-2)] p-1">
        {categoryOptions.map((category) => <Button size="sm" variant={categories.includes(category.id) ? "outline" : "ghost"} key={category.id} aria-pressed={categories.includes(category.id)} disabled={isRunning} onClick={() => cleaner.setCategory(category.id)} className="rounded-[var(--r-sm)] border-0 text-left">{category.label}</Button>)}
      </div>
      <Button size="sm" variant="outline" onClick={() => void cleaner.scanProjects()} disabled={isRunning}><Icon icon="folder-open" /> Choose project folder</Button>
      {!categories.length && <span className="text-xs text-[var(--warn)]">Choose at least one category.</span>}
    </div>
    {!scan && (
      <CacheScanIdleState
        isScanning={operation === "scanning"}
        onScan={() => void cleaner.scanSelected()}
        disabled={isRunning || !categories.length}
        label={scanLabel}
      />
    )}
    {operation === "cleaning" && <Card><CardContent className="flex items-center gap-3 py-4 text-sm text-[var(--text-dim)]"><Icon icon="clean" className="animate-spin text-[var(--accent)]" /> Cleaning selected items. You can cancel while the current target completes.</CardContent></Card>}
    {error && <ResultNotice tone="danger" title="Operation failed" message={error} />}
    {result && <CleanResult result={result} />}
    <RoutineCleanerPreview cleaner={cleaner} onRequestClean={() => setConfirmOpen(true)} />
    <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Clean selected cache data?</AlertDialogTitle>
          <AlertDialogDescription>
            WinCommander will clean {selectedItems.length} selected target{selectedItems.length === 1 ? "" : "s"}. The scan lists {formatBytes(selectedLogicalBytes)} of logical file data{selectedVacuumEstimate ? ` and ${formatBytes(selectedVacuumEstimate)} estimated reclaim from database optimization` : ""}. The database amount is an estimate, not recovered space; actual free-space change can differ. This cannot be undone.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <CleanupReviewItems items={reviewEntries} />
        <AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={() => void runClean()}>Clean selected</AlertDialogAction></AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </div>;
}

/** The owning card renders this in its header so refresh remains fixed to the card edge. */
export function RoutineCleanerHeaderActions({ cleaner }: { cleaner: RoutineCleaner }) {
  const { categories, operation, scan } = cleaner;
  const isRunning = operation !== "idle";
  const scanLabel = scan && cleaner.scanScope !== "projects" ? "Rescan application caches" : "Scan application caches";

  return (
    <div className="ml-auto flex shrink-0 items-center gap-2">
      {isRunning && (
        <Button size="icon" variant="outline" onClick={() => void cleaner.cancel()} title="Cancel cache operation" aria-label="Cancel cache operation">
          <Icon icon="stop" />
        </Button>
      )}
      <Button size="sm" variant="primary" onClick={() => void cleaner.scanSelected()} disabled={isRunning || !categories.length} title={scanLabel} aria-label={scanLabel}>
        <Icon icon={isRunning || scan ? "refresh" : "search"} className={operation === "scanning" ? "animate-spin" : undefined} /> {operation === "scanning" ? "Scanning…" : scan && cleaner.scanScope !== "projects" ? "Scan again" : "Scan caches"}
      </Button>
    </div>
  );
}

function CacheScanIdleState({ isScanning, onScan, disabled, label }: { isScanning: boolean; onScan: () => void; disabled: boolean; label: string }) {
  return (
    <div className="maintenance-cache-idle" role="status" aria-live="polite">
      <div className="maintenance-cache-idle__visual" aria-hidden="true">
        <span className="maintenance-cache-idle__orbit maintenance-cache-idle__orbit--outer" />
        <span className="maintenance-cache-idle__orbit maintenance-cache-idle__orbit--inner" />
        <span className="maintenance-cache-idle__sweep" />
        <span className="maintenance-cache-idle__core"><Icon icon="applications" /></span>
      </div>
      <div className="relative text-center">
        <p className="text-sm font-medium text-[var(--text)]">{isScanning ? "Checking application caches…" : "Ready to inspect application caches"}</p>
        <p className="mt-1 max-w-sm text-xs text-[var(--text-dim)]">{isScanning ? "Discovering regenerable data without changing any files." : "Choose the cache groups above, then scan to preview every target before cleaning."}</p>
        {!isScanning && (
          <Button size="sm" variant="primary" className="mt-3" onClick={onScan} disabled={disabled}>
            <Icon icon="search" /> {label}
          </Button>
        )}
      </div>
    </div>
  );
}

function CleanResult({ result }: { result: RoutineCleanerCleanResult }) {
  if (result.cancelled) return <ResultNotice tone="warning" title="Cleaning cancelled" message={`Removed ${formatBytes(result.bytesRecovered)} of logical data before cancellation.`} />;
  if (result.errors.length) return <ResultNotice tone="warning" title="Cleaning completed with issues" message={`${result.itemsCleaned} targets cleaned; ${result.errors.length} target${result.errors.length === 1 ? "" : "s"} could not be completed.`} />;
  return <ResultNotice tone="success" title="Cleaning complete" message={`Removed ${formatBytes(result.bytesRecovered)} of logical data from ${result.filesCleaned.toLocaleString()} files.`} />;
}

function ResultNotice({ tone, title, message }: { tone: "success" | "warning" | "danger"; title: string; message: string }) {
  return <Card className="border-[var(--border-strong)]"><CardContent className="flex items-start gap-3 py-4"><Badge tone={tone}>{title}</Badge><p className="text-sm text-[var(--text-dim)]">{message}</p></CardContent></Card>;
}
