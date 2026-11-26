// Pure report shaping for the Share Safely dialog: not-cleaned reasons and
// the per-queued-folder breakdown. Prefers the engine's per-file `files` /
// `roots` lists; falls back to bare `skippedFiles` paths from older engines.
import type { ScrubFileOutcome, ScrubReport, ScrubResult } from './scrubTypes';

export type SkipReasonId =
  | 'unsupported' | 'mismatch' | 'locked' | 'accessDenied' | 'readFailed' | 'writeFailed'
  | 'pathTooLong' | 'emptyFile' | 'nothingToRemove' | 'failed' | 'unreported' | 'other';

const REASON_LABELS: Record<SkipReasonId, string> = {
  unsupported: 'Unsupported format',
  mismatch: 'Type mismatch',
  locked: 'In use by another app',
  accessDenied: 'No permission',
  readFailed: "Couldn't read",
  writeFailed: "Couldn't write",
  pathTooLong: 'Path too long',
  emptyFile: 'Empty file — nothing to remove',
  nothingToRemove: 'Nothing to remove',
  failed: 'Could not be cleaned',
  // Engines that send bare paths only skip for these two reasons.
  unreported: 'Unsupported format or content mismatch',
  other: 'Other reason',
};

const REASON_ORDER: readonly SkipReasonId[] = [
  'locked', 'accessDenied', 'readFailed', 'writeFailed', 'pathTooLong', 'failed',
  'mismatch', 'unsupported', 'other', 'unreported', 'emptyFile', 'nothingToRemove',
];

const REASON_ALIASES: Record<string, SkipReasonId> = {
  unsupportedformat: 'unsupported', unsupported: 'unsupported',
  contentmismatch: 'mismatch', typemismatch: 'mismatch', mismatch: 'mismatch',
  locked: 'locked', inuse: 'locked', sharingviolation: 'locked',
  accessdenied: 'accessDenied', permissiondenied: 'accessDenied',
  readfailed: 'readFailed',
  writefailed: 'writeFailed',
  toolongpath: 'pathTooLong', pathtoolong: 'pathTooLong',
  emptyfile: 'emptyFile',
  nothingtoremove: 'nothingToRemove', alreadyclean: 'nothingToRemove',
  failed: 'failed', error: 'failed',
};

/** Accepts snake_case, kebab-case or camelCase reason codes. */
export function skipReasonId(code: string | null | undefined): SkipReasonId {
  if (!code || !code.trim()) return 'unreported';
  return REASON_ALIASES[code.toLowerCase().replace(/[^a-z]/g, '')] ?? 'other';
}

export function skipReasonLabel(id: SkipReasonId): string {
  return REASON_LABELS[id];
}

/** Reasons that mean "nothing needed doing" rather than a problem. */
export function isNeutralReason(id: SkipReasonId): boolean {
  return id === 'nothingToRemove' || id === 'emptyFile';
}

export interface NotCleanedEntry {
  path: string;
  reason: SkipReasonId;
  /** Row-specific explanation, e.g. "named .jpg but is really PNG". */
  note?: string;
  /** A selected folder that could not be listed, so its files can't be named. */
  isFolder?: boolean;
}

function outcomeNote(file: ScrubFileOutcome, reason: SkipReasonId): string | undefined {
  if (file.cleanedAs) return `cleaned as ${file.cleanedAs.toUpperCase()}`;
  if (reason === 'mismatch' && file.detectedType) {
    const named = file.extension ? `named .${file.extension} but ` : '';
    return `${named}is really ${file.detectedType.toUpperCase()}`;
  }
  if (file.message?.trim()) return file.message.trim();
  if (reason === 'other' && file.reason) return file.reason;
  return undefined;
}

/** A file belongs in the not-cleaned list when it has no file card to explain it. */
function isListedOutcome(file: ScrubFileOutcome): boolean {
  return file.outcome === 'not_cleaned' || file.outcome === 'failed' || (file.outcome === 'already_clean' && file.scrubbedIndex == null);
}

export function notCleanedEntries(report: Pick<ScrubReport, 'skippedFiles' | 'files'>): NotCleanedEntry[] {
  if (Array.isArray(report.files)) {
    return report.files.filter((f) => f && typeof f.path === 'string' && f.path && isListedOutcome(f)).map((f) => {
      const reason = skipReasonId(f.reason ?? (f.outcome === 'failed' ? 'failed' : null));
      const note = outcomeNote(f, reason);
      return { path: f.path, reason, ...(note ? { note } : {}), ...(f.detectedType === 'folder' ? { isFolder: true } : {}) };
    });
  }
  const raw = Array.isArray(report.skippedFiles) ? report.skippedFiles : [];
  return raw.filter((p): p is string => typeof p === 'string' && p.length > 0).map((path) => ({ path, reason: 'unreported' }));
}

export interface NotCleanedGroup {
  reason: SkipReasonId;
  label: string;
  entries: NotCleanedEntry[];
}

export function groupNotCleaned(entries: readonly NotCleanedEntry[]): NotCleanedGroup[] {
  return REASON_ORDER.flatMap((reason) => {
    const matching = entries.filter((e) => e.reason === reason);
    return matching.length > 0 ? [{ reason, label: REASON_LABELS[reason], entries: matching }] : [];
  });
}

function normalizePath(path: string): string {
  return path.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
}

/** The queued root that owns `path`; nested queued roots win over their parent. */
export function owningRoot(path: string, roots: readonly string[]): string | undefined {
  const target = normalizePath(path);
  let best: string | undefined;
  for (const root of roots) {
    const normalized = normalizePath(root);
    const isUnder = target === normalized || target.startsWith(`${normalized}\\`);
    if (isUnder && normalized.length > (best === undefined ? -1 : normalizePath(best).length)) best = root;
  }
  return best;
}

function lastSegment(path: string): string {
  return path.replace(/[/\\]+$/, '').split(/[/\\]/).pop() || path;
}

/** Parent folder shown next to a file, relative to its queued folder when it has one. */
export function displayFolder(path: string, roots: readonly string[]): string {
  const parent = path.replace(/[/\\][^/\\]*$/, '');
  const root = owningRoot(path, roots);
  if (!root || normalizePath(root) === normalizePath(path)) return parent;
  const relative = parent.slice(root.replace(/[/\\]+$/, '').length).replace(/^[/\\]+/, '');
  return relative ? `${lastSegment(root)}\\${relative}` : lastSegment(root);
}

export function hasRemovableData(result: ScrubResult): boolean {
  return (
    result.fieldsStripped.length > 0 ||
    !!result.gpsCoords ||
    (result.residualFields?.length ?? 0) > 0 ||
    (result.sampleValues?.length ?? 0) > 0
  );
}

export interface FolderBucket {
  /** Queued root path; empty for the merged "individual files" bucket. */
  root: string;
  name: string;
  isFolder: boolean;
  /** Files with a detail card. */
  results: ScrubResult[];
  total: number;
  /** Files with metadata to remove (preview) — includes `stillPresent`. */
  toClean: number;
  cleaned: number;
  nothingToRemove: number;
  notCleaned: number;
  stillPresent: number;
}

export const INDIVIDUAL_FILES_LABEL = 'Individual files';

type Counts = Omit<FolderBucket, 'root' | 'name' | 'isFolder'>;

function sumCounts(parts: readonly Counts[]): Counts {
  const sum = (key: Exclude<keyof Counts, 'results'>) => parts.reduce((n, p) => n + p[key], 0);
  return {
    results: parts.flatMap((p) => p.results),
    total: sum('total'), toClean: sum('toClean'), cleaned: sum('cleaned'),
    nothingToRemove: sum('nothingToRemove'), notCleaned: sum('notCleaned'), stillPresent: sum('stillPresent'),
  };
}

function finish(folders: FolderBucket[], loose: Counts[]): FolderBucket[] {
  const merged = sumCounts(loose);
  return merged.total > 0 || merged.results.length > 0
    ? [...folders, { root: '', name: INDIVIDUAL_FILES_LABEL, isFolder: false, ...merged }]
    : folders;
}

function aggregateFromEngine(report: Required<Pick<ScrubReport, 'files' | 'roots'>> & Pick<ScrubReport, 'scrubbed'>): FolderBucket[] {
  const folders: FolderBucket[] = [];
  const loose: Counts[] = [];
  for (const summary of report.roots) {
    const results = report.files
      .filter((f) => f.root === summary.root && f.scrubbedIndex != null)
      .map((f) => report.scrubbed[f.scrubbedIndex as number])
      .filter((r): r is ScrubResult => !!r);
    const counts: Counts = {
      results,
      total: summary.total,
      toClean: summary.cleaned + summary.residual,
      cleaned: summary.cleaned,
      nothingToRemove: summary.alreadyClean,
      notCleaned: summary.notCleaned + summary.failed,
      stillPresent: summary.residual,
    };
    if (summary.isDirectory) folders.push({ root: summary.root, name: lastSegment(summary.root), isFolder: true, ...counts });
    else loose.push(counts);
  }
  return finish(folders, loose);
}

function aggregateFromPaths(
  results: readonly ScrubResult[],
  notCleaned: readonly NotCleanedEntry[],
  roots: readonly string[],
  folderPaths: ReadonlySet<string>,
): FolderBucket[] {
  const keyOf = (path: string) => owningRoot(path, roots) ?? '';
  const hasDescendant = new Set(
    [...results.map((r) => r.inputPath), ...notCleaned.map((e) => e.path)]
      .map((path) => [path, keyOf(path)] as const)
      .filter(([path, root]) => root && normalizePath(root) !== normalizePath(path))
      .map(([, root]) => root),
  );
  const countsFor = (keys: ReadonlySet<string>): Counts => {
    const mine = results.filter((r) => keys.has(keyOf(r.inputPath)));
    const withData = mine.filter(hasRemovableData).length;
    const stillPresent = mine.filter((r) => (r.residualFields?.length ?? 0) > 0).length;
    const skipped = notCleaned.filter((e) => keys.has(keyOf(e.path))).length;
    return {
      results: mine, total: mine.length + skipped, toClean: withData, cleaned: withData - stillPresent,
      nothingToRemove: mine.length - withData, notCleaned: skipped, stillPresent,
    };
  };
  const folderRoots = roots.filter((root) => folderPaths.has(root) || hasDescendant.has(root));
  const folders = folderRoots.map((root) => ({ root, name: lastSegment(root), isFolder: true, ...countsFor(new Set([root])) }));
  const looseKeys = new Set(['', ...roots.filter((root) => !folderRoots.includes(root))]);
  return finish(folders, [countsFor(looseKeys)]);
}

/** Splits a report by queued folder. Loose queued files share one bucket. */
export function aggregateByRoot(
  report: Pick<ScrubReport, 'scrubbed' | 'files' | 'roots'>,
  notCleaned: readonly NotCleanedEntry[],
  roots: readonly string[],
  folderPaths: ReadonlySet<string>,
): FolderBucket[] {
  if (Array.isArray(report.files) && Array.isArray(report.roots) && report.roots.length > 0) {
    return aggregateFromEngine({ scrubbed: report.scrubbed, files: report.files, roots: report.roots });
  }
  return aggregateFromPaths(report.scrubbed, notCleaned, roots, folderPaths);
}

/** Sections only add information when the batch spans more than one place. */
export function shouldShowFolderSections(buckets: readonly FolderBucket[]): boolean {
  return buckets.length > 1 && buckets.some((b) => b.isFolder);
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}

export function folderStatusText(bucket: FolderBucket, isDryRun: boolean): string {
  const parts = [
    plural(bucket.total, 'file'),
    isDryRun ? `${bucket.toClean} to clean` : `${bucket.cleaned} cleaned`,
    `${bucket.nothingToRemove} nothing to remove`,
  ];
  if (bucket.notCleaned > 0) parts.push(`${bucket.notCleaned} not cleaned`);
  if (!isDryRun && bucket.stillPresent > 0) parts.push(`${bucket.stillPresent} still contain data`);
  return `${isDryRun ? 'Ready' : 'Done'} — ${parts.join(', ')}`;
}
