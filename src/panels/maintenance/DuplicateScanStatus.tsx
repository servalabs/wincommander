import type { DuplicateProgress, DuplicateScan } from "../../hooks/useBackend";
import { Badge } from "../../components/ui/badge";
import { toExplorerDisplayPath } from "./maintenancePathDisplay";
import { formatBytes } from "./routineCleanerHelpers";

function displayScanPath(path: string): string { return toExplorerDisplayPath(path) ?? "Path unavailable"; }
function displayScanError(entry: string): string {
  const delimiter = entry.lastIndexOf(": ");
  if (delimiter < 1) return entry.includes("\\\\?\\") ? "Path unavailable" : entry;
  const path = toExplorerDisplayPath(entry.slice(0, delimiter));
  return path ? `${path}: ${entry.slice(delimiter + 2)}` : `Path unavailable: ${entry.slice(delimiter + 2)}`;
}

export function DuplicateScanStatus({ progress, scan, scanning, local }: { progress?: DuplicateProgress; scan?: DuplicateScan; scanning: boolean; local: boolean }) {
  const state = scanning ? progress : scan;
  const roots = state?.scopeRoots ?? [];
  const exclusions = state?.excludedVolumes ?? [];
  const minimumSizeBytes = state?.minimumSizeBytes ?? 0;
  const skippedSmallFiles = state?.skippedSmallFiles ?? 0;
  const hasSafeExclusions = Boolean(
    scan && (scan.skippedEntries || scan.skippedLinks || scan.excludedVolumes.length),
  );
  return <div className="rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3 text-xs">
    <div className="flex flex-wrap items-center gap-2"><strong>{local ? "Current user profile" : "Selected folders"}</strong><Badge tone={scanning ? "accent" : scan?.cancelled || hasSafeExclusions ? "warning" : "success"}>{scanning ? progress?.phase === "hashing" ? "Comparing file contents" : "Discovering files" : scan ? scan.cancelled ? "Cancelled · partial results" : hasSafeExclusions ? "Scan finished · exclusions shown" : "Scan finished" : "Automatic read-only scan"}</Badge></div>
    <div className="mt-2 flex flex-wrap gap-2 text-[var(--text-dim)]">
      <span className="rounded border border-[var(--border)] px-2 py-1">{roots.length} scan location{roots.length === 1 ? "" : "s"}</span>
      {minimumSizeBytes > 0 && <span className="rounded border border-[var(--border)] px-2 py-1">{skippedSmallFiles.toLocaleString()} below {formatBytes(minimumSizeBytes)}</span>}
      {!!state?.skippedLinks && <span className="rounded border border-[var(--border)] px-2 py-1">{state.skippedLinks.toLocaleString()} links excluded</span>}
      {!!state?.skippedEntries && <span className="rounded border border-[var(--border)] px-2 py-1 text-[var(--warn)]">{state.skippedEntries.toLocaleString()} unavailable</span>}
    </div>
    {scanning && progress && <div role="status" aria-live="polite" className="mt-2 flex flex-col gap-1 text-[var(--text-dim)]">
      <span>{progress.scannedFiles.toLocaleString()} eligible files discovered{progress.phase === "hashing" ? ` · ${progress.hashedFiles.toLocaleString()} of ${progress.candidateFiles.toLocaleString()} same-size candidates processed · ${formatBytes(progress.bytesHashed)} read` : " · finding same-size candidates"}</span>
      {progress.currentPath && <span className="break-all font-mono text-[11px]">{displayScanPath(progress.currentPath)}</span>}
    </div>}
    <div className="mt-2 flex flex-wrap gap-x-5 gap-y-2">
      {roots.length > 0 && <details><summary className="cursor-pointer">Scan locations ({roots.length})</summary><ul aria-label="Duplicate scan scope" className="mt-2 max-h-40 overflow-auto space-y-1 font-mono">{roots.map((root, index) => <li key={`${root}-${index}`} className="break-all">{displayScanPath(root)}</li>)}</ul></details>}
      {!!exclusions.length && <details><summary className="cursor-pointer">Excluded drives ({exclusions.length})</summary><ul className="mt-2 max-h-40 overflow-auto space-y-1">{exclusions.map((entry) => <li key={entry} className="break-all">{entry}</li>)}</ul></details>}
      {!!state?.errors.length && <details className="text-[var(--warn)]"><summary className="cursor-pointer">Unavailable locations (up to 20 examples)</summary><ul className="mt-2 max-h-40 overflow-auto space-y-1">{state.errors.map((entry, index) => <li key={index} className="break-all font-mono text-[11px]">{displayScanError(entry)}</li>)}</ul></details>}
    </div>
  </div>;
}
