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
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Chip } from "../../components/ui/chip";
import { Icon } from "../../components/ui/icon";
import type { RoutineCleanerCategory, RoutineCleanerCleanResult } from "../../hooks/useBackend";
import { showError, showSuccess } from "../../utils/toast";
import { formatBytes, getRoutineCleanerCategories } from "./routineCleanerHelpers";
import { RoutineCleanerPreview } from "./RoutineCleanerPreview";
import { useRoutineCleaner } from "./useRoutineCleaner";

interface RoutineCleanerPanelProps {
  categories: RoutineCleanerCategory[];
  title: string;
  description: string;
}

/** Reusable preview-and-confirm cache cleaner. The caller explicitly owns its categories. */
export function RoutineCleanerPanel({ categories: allowedCategories, title, description }: RoutineCleanerPanelProps) {
  const cleaner = useRoutineCleaner(allowedCategories);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const { categories, operation, scan, selectedItems, result, error } = cleaner;
  const isRunning = operation !== "idle";
  const categoryOptions = getRoutineCleanerCategories(allowedCategories);
  const scanLabel = scan ? "Rescan application caches" : "Scan application caches";

  useEffect(() => {
    if (error) void showError(error);
  }, [error]);

  useEffect(() => {
    if (result && !result.cancelled && !result.errors.length) {
      void showSuccess(`Recovered ${formatBytes(result.bytesRecovered)} from ${result.filesCleaned.toLocaleString()} files.`);
    }
  }, [result]);

  const runClean = async () => {
    setConfirmOpen(false);
    await cleaner.cleanSelected();
  };

  return <div className="maintenance-cache-cleaner flex flex-col gap-4">
    <Card className="maintenance-cache-card flex flex-1 flex-col">
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-1 flex-col gap-4">
        {categoryOptions.length > 1 && <div className="flex flex-wrap gap-2">
          {categoryOptions.map((category) => <Chip key={category.id} active={categories.includes(category.id)} onClick={() => cleaner.setCategory(category.id)} className="text-left">{category.label}</Chip>)}
        </div>}
        <div className="flex flex-wrap items-center gap-2">
          <Button size="icon" variant="primary" onClick={() => void cleaner.scanSelected()} disabled={isRunning || !categories.length} title={scanLabel} aria-label={scanLabel}>
            <Icon icon={isRunning || scan ? "refresh" : "search"} className={operation === "scanning" ? "animate-spin" : undefined} />
          </Button>
          {isRunning && <Button size="icon" variant="outline" onClick={() => void cleaner.cancel()} title="Cancel cache operation" aria-label="Cancel cache operation"><Icon icon="stop" /></Button>}
          {!categories.length && <span className="text-xs text-[var(--warn)]">Choose at least one category.</span>}
        </div>
        {!scan && <CacheScanIdleState isScanning={operation === "scanning"} />}
      </CardContent>
    </Card>
    {operation === "cleaning" && <Card><CardContent className="flex items-center gap-3 py-4 text-sm text-[var(--text-dim)]"><Icon icon="clean" className="animate-spin text-[var(--accent)]" /> Cleaning selected items. You can cancel while the current target completes.</CardContent></Card>}
    {error && <ResultNotice tone="danger" title="Operation failed" message={error} />}
    {result && <CleanResult result={result} />}
    <RoutineCleanerPreview cleaner={cleaner} onRequestClean={() => setConfirmOpen(true)} />
    <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
      <AlertDialogContent><AlertDialogHeader><AlertDialogTitle>Clean selected cache data?</AlertDialogTitle><AlertDialogDescription>WinCommander will clean {selectedItems.length} selected target{selectedItems.length === 1 ? "" : "s"} and attempt to recover {formatBytes(selectedItems.reduce((total, item) => total + item.bytes, 0))}. This cannot be undone.</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={() => void runClean()}>Clean selected</AlertDialogAction></AlertDialogFooter></AlertDialogContent>
    </AlertDialog>
  </div>;
}

function CacheScanIdleState({ isScanning }: { isScanning: boolean }) {
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
      </div>
    </div>
  );
}

function CleanResult({ result }: { result: RoutineCleanerCleanResult }) {
  if (result.cancelled) return <ResultNotice tone="warning" title="Cleaning cancelled" message={`Recovered ${formatBytes(result.bytesRecovered)} before cancellation.`} />;
  if (result.errors.length) return <ResultNotice tone="warning" title="Cleaning completed with issues" message={`${result.itemsCleaned} targets cleaned; ${result.errors.length} target${result.errors.length === 1 ? "" : "s"} could not be completed.`} />;
  return <ResultNotice tone="success" title="Cleaning complete" message={`Recovered ${formatBytes(result.bytesRecovered)} from ${result.filesCleaned.toLocaleString()} files.`} />;
}

function ResultNotice({ tone, title, message }: { tone: "success" | "warning" | "danger"; title: string; message: string }) {
  return <Card className="border-[var(--border-strong)]"><CardContent className="flex items-start gap-3 py-4"><Badge tone={tone}>{title}</Badge><p className="text-sm text-[var(--text-dim)]">{message}</p></CardContent></Card>;
}
