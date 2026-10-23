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
    <div className="flex flex-wrap items-center gap-2"><strong>{local ? "All eligible local fixed drive letters" : "Selected folders"}</strong><Badge tone={scanning ? "accent" : scan?.cancelled || hasSafeExclusions ? "warning" : "success"}>{scanning ? progress?.phase === "hashing" ? "Comparing file contents" : "Discovering files" : scan ? scan.cancelled ? "Cancelled · partial results" : hasSafeExclusions ? "Scan finished · exclusions shown" : "Scan finished" : "Automatic read-only scan"}</Badge></div>
    {roots.length > 0 && <ul aria-label="Duplicate scan scope" className="mt-2 flex flex-wrap gap-x-4 gap-y-1 font-mono">{roots.map((root, index) => <li key={`${root}-${index}`} className="break-all">{displayScanPath(root)}</li>)}</ul>}
    {scanning && progress && <div role="status" aria-live="polite" className="mt-2 flex flex-col gap-1 text-[var(--text-dim)]">
      <span>{progress.scannedFiles.toLocaleString()} eligible files discovered{progress.phase === "hashing" ? ` · ${progress.hashedFiles.toLocaleString()} of ${progress.candidateFiles.toLocaleString()} same-size candidates processed · ${formatBytes(progress.bytesHashed)} read` : " · finding same-size candidates"}</span>
      {minimumSizeBytes > 0 && <span>{skippedSmallFiles.toLocaleString()} files below {formatBytes(minimumSizeBytes)} skipped for this scan</span>}
      <span>{progress.skippedEntries.toLocaleString()} skipped entries · {progress.skippedLinks.toLocaleString()} links excluded</span>
      {progress.currentPath && <span className="break-all font-mono text-[11px]">{displayScanPath(progress.currentPath)}</span>}
    </div>}
    {!scanning && minimumSizeBytes > 0 && <p className="mt-2 text-[var(--text-dim)]">{skippedSmallFiles.toLocaleString()} files below {formatBytes(minimumSizeBytes)} were excluded by the selected size rule.</p>}
    <p className="mt-2 text-[var(--text-dim)]">Scanning only reads files. Protected system/application locations, links, directory-mounted volumes and inaccessible entries remain outside the existing safe scan policy. Network and removable drives are opt-in through Choose folders; linked roots remain unsupported.</p>
    {!scanning && !scan?.cancelled && hasSafeExclusions && <p className="mt-2 text-[var(--text-dim)]">The scan finished for every location WinCommander is allowed to inspect. The details below list locations excluded for safety or because Windows did not make them available.</p>}
    {!!state?.skippedLinks && <p className="mt-2 text-[var(--text-dim)]">{state.skippedLinks.toLocaleString()} linked entries excluded.</p>}
    {!!exclusions.length && <details className="mt-2"><summary className="cursor-pointer">Drives outside automatic scope ({exclusions.length})</summary><ul className="mt-2 space-y-1">{exclusions.map((entry) => <li key={entry} className="break-all">{entry}</li>)}</ul></details>}
    {!!state?.errors.length && <details className="mt-2 text-[var(--warn)]"><summary className="cursor-pointer">Skipped/unavailable locations ({state.skippedEntries.toLocaleString()}; up to 20 examples)</summary><ul className="mt-2 space-y-1">{state.errors.map((entry, index) => <li key={index} className="break-all font-mono text-[11px]">{displayScanError(entry)}</li>)}</ul></details>}
  </div>;
}
