import type { DuplicateProgress, DuplicateScan } from "../../hooks/useBackend";
import { Badge } from "../../components/ui/badge";
import { formatBytes } from "./routineCleanerHelpers";

export function DuplicateScanStatus({ progress, scan, scanning, local }: { progress?: DuplicateProgress; scan?: DuplicateScan; scanning: boolean; local: boolean }) {
  const state = scanning ? progress : scan;
  const roots = state?.scopeRoots ?? [];
  const exclusions = state?.excludedVolumes ?? [];
  return <div className="rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3 text-xs">
    <div className="flex flex-wrap items-center gap-2"><strong>{local ? "All eligible local fixed drive letters" : "Selected folders"}</strong><Badge tone={scanning ? "accent" : scan?.complete ? "success" : "warning"}>{scanning ? progress?.phase === "hashing" ? "Comparing file contents" : "Discovering files" : scan ? scan.cancelled ? "Cancelled · partial results" : scan.complete ? "Scan finished" : "Scan finished with exclusions" : "Automatic read-only scan"}</Badge></div>
    {roots.length > 0 && <ul aria-label="Duplicate scan scope" className="mt-2 flex flex-wrap gap-x-4 gap-y-1 font-mono">{roots.map((root) => <li key={root} className="break-all">{root}</li>)}</ul>}
    {scanning && progress && <div role="status" aria-live="polite" className="mt-2 flex flex-col gap-1 text-[var(--text-dim)]">
      <span>{progress.scannedFiles.toLocaleString()} unique files discovered{progress.phase === "hashing" ? ` · ${progress.hashedFiles.toLocaleString()} of ${progress.candidateFiles.toLocaleString()} same-size candidates processed · ${formatBytes(progress.bytesHashed)} read` : " · finding same-size candidates"}</span>
      <span>{progress.skippedEntries.toLocaleString()} skipped entries · {progress.skippedLinks.toLocaleString()} links excluded</span>
      <span className="break-all font-mono text-[11px]">{progress.currentPath}</span>
    </div>}
    <p className="mt-2 text-[var(--text-dim)]">Scanning only reads files. Protected system/application locations, links, directory-mounted volumes and inaccessible entries remain outside the existing safe scan policy. Network and removable drives are opt-in through Choose folders; linked roots remain unsupported.</p>
    {!!state?.skippedLinks && <p className="mt-2 text-[var(--text-dim)]">{state.skippedLinks.toLocaleString()} linked entries excluded.</p>}
    {!!exclusions.length && <details className="mt-2"><summary className="cursor-pointer">Drives outside automatic scope ({exclusions.length})</summary><ul className="mt-2 space-y-1">{exclusions.map((entry) => <li key={entry} className="break-all">{entry}</li>)}</ul></details>}
    {!!state?.errors.length && <details className="mt-2 text-[var(--warn)]"><summary className="cursor-pointer">Skipped/unavailable locations ({state.skippedEntries.toLocaleString()}; up to 20 examples)</summary><ul className="mt-2 space-y-1">{state.errors.map((entry, index) => <li key={index} className="break-all font-mono text-[11px]">{entry}</li>)}</ul></details>}
  </div>;
}
