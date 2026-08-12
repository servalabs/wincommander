// src/components/MetadataScrubberDialog.tsx
//
// "Share Safely" — strip metadata from common file types before
// sharing. Backed by the Rust `scrub_metadata_paths` Tauri command in
// `metadata_scrubber.rs`. Two operator entry points:
//
//   1. Pick-files button → native file picker (multi-select).
//   2. Drag-drop zone → Tauri webview `onDrop` listener.
//
// Output defaults to `<source-dir>/_scrubbed/` so the operator can
// drag-drop that whole folder elsewhere.
//
// We deliberately render no preview of the stripped data — the point is
// "share without leaking", not "see what you were leaking". A future
// audit panel can show that.
//
// Lives in `components/` because it's launched from the global Right
// Sidebar — not bound to any one panel.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { Button, Classes, Dialog, Icon, ProgressBar, Switch, Tag } from '@/components/ui/bp';
import { useTheme } from '../context/ThemeContext';
import { useAppState } from '../context/AppContext';
import useDependencies from '../hooks/useDependencies';

// Resolved once at module init — used as <base href> inside srcDoc iframes so
// that relative paths like /leaflet/leaflet.js resolve correctly from about:srcdoc.
const MAP_ORIGIN = window.location.origin;

interface StrippedField {
  /** Stable category id — drives chip colour + summary roll-up. */
  category: string;
  /** Friendly chip label, e.g. "ICC Color Profile". */
  label: string;
  /** Bytes of the segment / chunk removed. */
  bytes: number;
  /** True iff the stripped data typically identifies the operator
   *  (GPS, camera serial, author name, dates). Drives red highlight. */
  isIdentifying: boolean;
}

interface GpsCoords {
  lat: number;
  lon: number;
  /** Human-friendly "37.7749° N, 122.4194° W" string. */
  label: string;
}

interface ScrubResult {
  inputPath: string;
  outputPath: string;
  fileType: string;
  bytesIn: number;
  bytesOut: number;
  fieldsStripped: StrippedField[];
  /** Present iff the pre-scan extracted valid GPS from the file. */
  gpsCoords?: GpsCoords;
  /** Up to 6 sample values for the per-file tooltip / preview. */
  sampleValues?: string[];
  /** Marker that this row came from a dry-run preview. */
  dryRun?: boolean;
  /** Identifying metadata STILL PRESENT after the scrub (survivors), plus
   *  can't-fully-remove advisories (e.g. a PDF we couldn't rewrite). Non-empty
   *  means this file is NOT safe to share. */
  residualFields?: StrippedField[];
}

interface ScrubProgress {
  current: number;
  total: number;
  file: string;
  dryRun?: boolean;
  done?: boolean;
  /** Queued root (folder/file path) this event belongs to — matches an entry
   *  in `selectedPaths`, so we can advance that item's own bar. */
  group?: string;
  /** Files finished so far within `group`. */
  groupCurrent?: number;
  /** Total supported files in `group` (known up front). */
  groupTotal?: number;
  /** Up-front plan: queued-root path → its total file count. Sent once before
   *  the first file finishes so every row can render "0 / N" immediately. */
  groups?: Record<string, number>;
}

/** Theme tint per category. Used for chip background + the stacked bar.
 *  Pulls from CSS vars so dark/light + custom themes Just Work. */
const CATEGORY_TINTS: Record<string, { bg: string; fg: string; border: string }> = {
  exif: tint('var(--color-danger)'),
  xmp: tint('var(--color-danger)'),
  iptc: tint('var(--color-danger)'),
  thumbnail: tint('var(--color-danger)'),
  pngText: tint('var(--color-danger)'),
  pngExif: tint('var(--color-danger)'),
  pdfInfo: tint('var(--color-danger)'),
  pdfXmp: tint('var(--color-danger)'),
  officeCore: tint('var(--color-danger)'),
  officeApp: tint('var(--color-danger)'),
  iccProfile: tint('var(--color-info, #3b82f6)'),
  pictureInfo: tint('var(--color-info, #3b82f6)'),
  adobe: tint('var(--color-info, #3b82f6)'),
  pngTime: tint('var(--color-warning)'),
  // Paranoid-mode passes — purple-ish to stand out from the regular
  // exiftool category chips. These aren't "data found in the file" so
  // much as "we did this extra thing to it."
  paranoid: tint('var(--color-accent)'),
  other: tint('var(--color-text-muted)'),
};

function tint(color: string) {
  return {
    bg: `color-mix(in srgb, ${color} 15%, transparent)`,
    fg: color,
    border: `color-mix(in srgb, ${color} 35%, transparent)`,
  };
}

function categoryTint(cat: string) {
  return CATEGORY_TINTS[cat] ?? CATEGORY_TINTS.other;
}

/** A single-line tinted chip describing one stripped category. Two sizes:
 *  `sm` for per-file rows (smaller, denser); `md` for the batch
 *  rollup (slightly bigger to draw the eye). `whiteSpace: nowrap` keeps
 *  long labels on one line — the rare overflow truncates with an
 *  ellipsis and the full label is in the tooltip. */
function CategoryChip({
  category,
  label,
  bytes,
  isIdentifying,
  size,
}: {
  category: string;
  label: string;
  bytes: number;
  isIdentifying: boolean;
  size: 'sm' | 'md';
}) {
  const t = categoryTint(category);
  const isMd = size === 'md';
  return (
    <span
      title={`${label} — ${formatBytes(bytes)}${isIdentifying ? ' · could identify you' : ''}`}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        fontSize: isMd ? 11 : 10,
        padding: isMd ? '2px 8px' : '1px 6px',
        borderRadius: 3,
        background: t.bg,
        color: t.fg,
        border: `1px solid ${t.border}`,
        whiteSpace: 'nowrap',
        maxWidth: isMd ? 220 : 180,
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
    >
      {isIdentifying && (
        <Icon
          icon="shield"
          size={isMd ? 10 : 9}
          style={{ flexShrink: 0, opacity: 0.85 }}
        />
      )}
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>
        {label}
      </span>
      <span style={{ opacity: 0.65, flexShrink: 0 }}>· {formatBytes(bytes)}</span>
    </span>
  );
}

interface ScrubError {
  inputPath: string;
  message: string;
}

interface ScrubReport {
  scrubbed: ScrubResult[];
  errors: ScrubError[];
  totalInputBytes: number;
  totalOutputBytes: number;
  skippedCount?: number;
  /** Paths of files that were NOT processed (unsupported format / content
   *  mismatch). Named so the user knows exactly what was left un-cleaned. */
  skippedFiles?: string[];
  /** Files that still carry identifying metadata after the scrub. Non-zero
   *  drives the red "still present — not safe to share" warning. */
  residualCount?: number;
}

// Reasonable subset of formats ExifTool can write to. ExifTool itself
// supports many more — the file picker filter is a hint, not a
// gatekeeper (operators can pick "All files" and ExifTool decides).
const SUPPORTED_EXTENSIONS = [
  // Images
  'jpg', 'jpeg', 'jpe', 'png', 'heic', 'heif', 'webp', 'avif',
  'tiff', 'tif', 'gif', 'bmp', 'jp2', 'jxl', 'mpo',
  // Adobe / design
  'psd', 'psb', 'ai', 'eps', 'ps',
  // RAW camera
  'cr2', 'cr3', 'crw', 'nef', 'nrw', 'arw', 'sr2', 'dng', 'rw2',
  'orf', 'ori', 'raf', 'pef', 'srw', 'rwl', 'erf', '3fr', 'fff',
  'iiq', 'x3f', 'mef', 'mos', 'mrw', 'gpr',
  // Documents
  'pdf',
  // Office
  'docx', 'xlsx', 'pptx',
  // Video
  'mp4', 'mov', 'm4v', '3gp', '3g2', 'mkv', 'webm',
  // Audio
  'mp3', 'm4a', 'flac', 'wav',
];

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

function basename(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

/** True iff `file` lives under (or is) the queued root `root`. Used to group the
 *  final per-file results back under each queued folder for the report. */
function isUnderRoot(file: string, root: string): boolean {
  return file === root || file.startsWith(`${root}\\`) || file.startsWith(`${root}/`);
}

function parseSampleValue(s: string): { key: string; value: string } {
  const idx = s.indexOf(':');
  if (idx > 0 && idx < 50) {
    const rawKey = s.slice(0, idx).trim();
    const cleanKey = rawKey
      .replace(/^GPS\s+/i, '')
      .replace(/([a-z])([A-Z])/g, '$1 $2')
      .replace(/([A-Z]{2,})([A-Z][a-z])/g, '$1 $2');
    return { key: cleanKey, value: s.slice(idx + 1).trim() };
  }
  return { key: '', value: s };
}

interface MetadataScrubberDialogProps {
  isOpen: boolean;
  onClose: () => void;
  /** Files pre-queued from a right-click context-menu launch. When
   *  supplied, the dialog opens with these in the "queued" list ready
   *  to scrub. Cleared after each fresh open so the same list doesn't
   *  re-seed on the next manual open. */
  initialPaths?: string[];
  /** Safe Paste opens the dialog on the FRESH COPIES it just made and
   *  wants them scrubbed in place (no `_scrubbed/` subfolder), so it
   *  forces Replace mode on. */
  initialReplaceMode?: boolean;
}

export default function MetadataScrubberDialog({
  isOpen,
  onClose,
  initialPaths,
  initialReplaceMode,
}: MetadataScrubberDialogProps) {
  const { theme } = useTheme();
  // Engine presence comes from the app-wide dependency cache — probed ONCE at
  // startup (AppContext.initializeApp → refreshDependencies) and file-cached, so
  // opening this dialog no longer re-checks the engine every time. The dashboard
  // "engines" indicator reads the same cache, so both stay consistent.
  const { dependencyStatus, refreshDependencies, forceRefreshDeps } = useAppState();
  const scrubberDep = dependencyStatus?.find((d) => d.id === 'metadataScrubber');
  const engineKnown = dependencyStatus != null;
  const engineInstalled = scrubberDep?.installed === true;
  const engineVersion = scrubberDep?.version ?? '';
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<ScrubReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dragHover, setDragHover] = useState(false);
  // Per-queued-item progress: { done, total } for each queued root, keyed by the
  // exact path the backend echoes back as `group`. Drives the parallel inline
  // bars; the final 100% tick flips `scrubDone`.
  const [progressByRoot, setProgressByRoot] = useState<Record<string, { done: number; total: number }>>({});
  const [scrubDone, setScrubDone] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const { installDependency } = useDependencies();
  // Paranoid mode — single user-facing toggle that turns on BOTH the
  // backend paranoid passes. Off by default; they're irreversible
  // (random timestamps can't be recovered) so we never make them the
  // implicit baseline.
  const [paranoidMode, setParanoidMode] = useState(false);
  const [replaceMode, setReplaceMode] = useState(false);
  // Tracks which queued paths were added as directories (via Pick folder…)
  // so the queue list can render a folder icon for them.
  const [folderPaths, setFolderPaths] = useState<Set<string>>(new Set());

  // Reset transient state when reopened. Engine presence is read from the
  // shared dependency cache (not re-probed here); install refreshes that cache.
  useEffect(() => {
    if (isOpen) {
      setReport(null);
      setError(null);
      setRunning(false);
      setProgressByRoot({});
      setScrubDone(false);
      // Seed from right-click paths if present; otherwise start empty.
      setSelectedPaths(initialPaths && initialPaths.length > 0 ? [...initialPaths] : []);
      // Safe Paste seeds fresh copies and wants them scrubbed in place.
      if (initialReplaceMode) setReplaceMode(true);
    }
  }, [isOpen, initialPaths, initialReplaceMode]);

  // Self-heal the rare cold-open case where the startup dependency probe hasn't
  // landed yet — populate the cache silently rather than re-checking on every open.
  useEffect(() => {
    if (isOpen && dependencyStatus == null) {
      void refreshDependencies();
    }
  }, [isOpen, dependencyStatus, refreshDependencies]);

  // Progress events from the backend during scrub. The engine sends an up-front
  // plan ({ groups: root → total }), then one event per finished file carrying
  // its group + running count, then a final done tick. We track { done, total }
  // per queued root so every folder/file fills its OWN bar in parallel — instead
  // of one shared bar alternating between folders.
  useEffect(() => {
    if (!isOpen) return;
    let unlisten: (() => void) | undefined;
    listen<ScrubProgress>('scrub-progress', (e) => {
      const p = e.payload;
      if (p.done) {
        setScrubDone(true);
        return;
      }
      if (p.groups) {
        const groups = p.groups;
        setProgressByRoot((prev) => {
          const next = { ...prev };
          for (const [root, total] of Object.entries(groups)) {
            next[root] = { done: next[root]?.done ?? 0, total };
          }
          return next;
        });
        return;
      }
      if (p.group) {
        const root = p.group;
        const groupCurrent = p.groupCurrent;
        const groupTotal = p.groupTotal;
        setProgressByRoot((prev) => ({
          ...prev,
          [root]: {
            done: groupCurrent ?? (prev[root]?.done ?? 0) + 1,
            total: groupTotal ?? prev[root]?.total ?? 0,
          },
        }));
      }
    }).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, [isOpen]);


  // Install the Hidden Data Remover dep directly from the dialog —
  // matches the pattern other deps use (winget under the hood). After
  // install, re-probe so the dialog flips from the install banner to
  // the dropzone without an app restart.
  const handleInstall = useCallback(async () => {
    setInstalling(true);
    setInstallError(null);
    try {
      const res = await installDependency('metadataScrubber');
      if (res.error || (res.data && (res.data as { success?: boolean }).success === false)) {
        const msg =
          (res.data as { message?: string })?.message ||
          res.error ||
          'Install failed';
        throw new Error(msg);
      }
      // Refresh the app-wide dependency cache (force) so BOTH this dialog and
      // the dashboard "engines" indicator flip to installed without a restart.
      // winget can take a few seconds to make the binary resolvable via PATH.
      await forceRefreshDeps();
    } catch (e) {
      setInstallError(String(e));
    } finally {
      setInstalling(false);
    }
  }, [installDependency, forceRefreshDeps]);

  // Tauri webview drop listener — picks up files dragged onto the
  // section from File Explorer. We only listen while the dialog is open
  // so other panels don't accidentally consume drops.
  useEffect(() => {
    if (!isOpen) return;
    let unlisten: (() => void) | undefined;
    listen<{ paths: string[] }>('tauri://drag-drop', (event) => {
      const paths = event.payload?.paths ?? [];
      if (paths.length === 0) return;
      setSelectedPaths((prev) => Array.from(new Set([...prev, ...paths])));
      setDragHover(false);
    }).then((u) => {
      unlisten = u;
    });
    const dragOverUnlistenP = listen('tauri://drag-over', () => setDragHover(true));
    const dragLeaveUnlistenP = listen('tauri://drag-leave', () => setDragHover(false));
    return () => {
      unlisten?.();
      dragOverUnlistenP.then((u) => u());
      dragLeaveUnlistenP.then((u) => u());
    };
  }, [isOpen]);

  const handlePickFiles = useCallback(async () => {
    try {
      const result = await open({
        multiple: true,
        directory: false,
        filters: [
          {
            name: 'Supported',
            extensions: SUPPORTED_EXTENSIONS,
          },
        ],
      });
      if (!result) return;
      const paths = Array.isArray(result) ? result : [result];
      setSelectedPaths((prev) => Array.from(new Set([...prev, ...paths])));
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handlePickFolder = useCallback(async () => {
    try {
      const result = await open({ multiple: false, directory: true });
      if (!result) return;
      const path = Array.isArray(result) ? result[0] : result;
      setSelectedPaths((prev) => Array.from(new Set([...prev, path])));
      setFolderPaths((prev) => new Set([...prev, path]));
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleClear = useCallback(() => {
    setSelectedPaths([]);
    setFolderPaths(new Set());
    setReport(null);
    setError(null);
  }, []);

  const handleRemovePath = useCallback((path: string) => {
    setSelectedPaths((prev) => prev.filter((p) => p !== path));
    setFolderPaths((prev) => {
      const next = new Set(prev);
      next.delete(path);
      return next;
    });
  }, []);

  const runScrub = useCallback(
    async (dryRun: boolean) => {
      if (selectedPaths.length === 0) return;
      setRunning(true);
      setError(null);
      setReport(null);
      setProgressByRoot({});
      setScrubDone(false);
      try {
        const r = await invoke<ScrubReport>('scrub_metadata_paths', {
          paths: selectedPaths,
          options: {
            dryRun,
            recursive: true,
            replaceOriginals: replaceMode,
            paranoid: {
              randomizeTimestamps: paranoidMode,
              stripAltStreams: paranoidMode,
            },
          },
        });
        setReport(r);
      } catch (err) {
        setError(String(err));
      } finally {
        setRunning(false);
      }
    },
    [paranoidMode, replaceMode, selectedPaths],
  );

  const handleScrub = useCallback(() => runScrub(false), [runScrub]);
  const handlePreview = useCallback(() => runScrub(true), [runScrub]);

  const scrubOutputDir = (() => {
    if (!report || report.scrubbed.every((r) => r.dryRun)) return null;
    const first = report.scrubbed.find((r) => !r.dryRun && r.outputPath);
    if (!first) return null;
    const p = first.outputPath;
    const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return idx > 0 ? p.slice(0, idx) : p;
  })();

  return (
    <Dialog
      isOpen={isOpen}
      onClose={onClose}
      title="Share Safely — Metadata Scrubber"
      icon="clean"
      className={`wc-dialog ${theme === 'dark' ? Classes.DARK : ''}`}
      style={{ width: 780, maxWidth: '96vw', maxHeight: '88vh', display: 'flex', flexDirection: 'column' }}
    >
      <div className="wc-dialog-body" style={{ padding: 20, overflowY: 'auto', flex: 1, minHeight: 0 }}>
        {engineKnown && !engineInstalled && (
          <ScrubberMissingBanner
            onInstall={handleInstall}
            installing={installing}
            installError={installError}
          />
        )}

        {engineInstalled && (
          <>
            {/* ── Drop zone ─────────────────────────────────────────── */}
            <div
              onDragOver={(e) => { e.preventDefault(); setDragHover(true); }}
              onDragLeave={() => setDragHover(false)}
              onDrop={(e) => { e.preventDefault(); setDragHover(false); }}
              style={{
                padding: '24px 20px 20px',
                border: `2px dashed ${dragHover ? 'var(--color-accent)' : 'var(--color-border)'}`,
                borderRadius: 8,
                background: dragHover
                  ? 'color-mix(in srgb, var(--color-accent) 5%, var(--color-bg-secondary))'
                  : 'var(--color-bg-secondary)',
                textAlign: 'center',
                transition: 'border-color 0.15s, background 0.15s',
              }}
            >
              <Icon
                icon="cloud-upload"
                size={28}
                style={{
                  color: dragHover ? 'var(--color-accent)' : 'var(--color-text-muted)',
                  marginBottom: 10,
                  display: 'block',
                  margin: '0 auto 10px',
                  transition: 'color 0.15s',
                }}
              />
              <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--color-text-primary)', marginBottom: 4 }}>
                Drop files or a folder here
              </div>
              <div style={{ fontSize: 11, color: 'var(--color-text-muted)', marginBottom: 14 }}>
                GPS, camera info, author name, timestamps — all removed. Originals untouched.
              </div>
              <div style={{ display: 'flex', gap: 8, justifyContent: 'center', flexWrap: 'wrap' }}>
                <Button icon="document" onClick={handlePickFiles} outlined small>
                  Pick files…
                </Button>
                <Button icon="folder-close" onClick={handlePickFolder} outlined small>
                  Pick folder…
                </Button>
              </div>
              {engineVersion && (
                <div style={{ marginTop: 14, fontSize: 10, color: 'var(--color-text-muted)' }}>
                  Hidden Data Remover v{engineVersion} · JPEG, PNG, HEIC, PDF, Office, RAW, Photoshop, MP4 and more
                </div>
              )}
            </div>

            {/* ── Queued list ───────────────────────────────────────── */}
            {selectedPaths.length > 0 && !report && (
              <div
                style={{
                  marginTop: 10,
                  border: '1px solid var(--color-border)',
                  borderRadius: 6,
                  overflow: 'hidden',
                }}
              >
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    padding: '6px 10px',
                    background: 'var(--color-bg-secondary)',
                    borderBottom: '1px solid var(--color-border)',
                    fontSize: 11,
                  }}
                >
                  <span style={{ fontWeight: 600, color: 'var(--color-text-muted)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
                    Queued · {selectedPaths.length}
                  </span>
                  <Button icon="cross" minimal small onClick={handleClear} style={{ minHeight: 0 }}>
                    Clear
                  </Button>
                </div>
                <div
                  style={{
                    maxHeight: 130,
                    overflowY: 'auto',
                    padding: '4px 0',
                  }}
                >
                  {selectedPaths.map((p) => (
                    <div
                      key={p}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 6,
                        padding: '3px 10px',
                        fontSize: 11,
                        fontFamily: 'var(--font-mono)',
                      }}
                    >
                      <Icon
                        icon={folderPaths.has(p) ? 'folder-close' : 'document'}
                        size={11}
                        color="var(--color-text-muted)"
                      />
                      <span
                        style={{
                          flex: 1,
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}
                        title={p}
                      >
                        {basename(p)}
                      </span>
                      {running ? (
                        <ScrubItemProgress progress={progressByRoot[p]} allDone={scrubDone} />
                      ) : (
                        <>
                          {folderPaths.has(p) && (
                            <Tag minimal style={{ fontSize: 9, padding: '0 4px' }}>folder</Tag>
                          )}
                          <button
                            onClick={() => handleRemovePath(p)}
                            title="Remove"
                            style={{
                              background: 'none',
                              border: 'none',
                              cursor: 'pointer',
                              color: 'var(--color-text-muted)',
                              padding: '0 2px',
                              lineHeight: 1,
                              fontSize: 13,
                              flexShrink: 0,
                            }}
                          >
                            ×
                          </button>
                        </>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* ── Options row: Paranoid mode + Replace originals ───── */}
            <div style={{ marginTop: 10, display: 'flex', gap: 8 }}>
              {/* Paranoid mode */}
              <div
                style={{
                  flex: 1,
                  border: `1px solid ${paranoidMode ? 'color-mix(in srgb, var(--color-accent) 40%, transparent)' : 'var(--color-border)'}`,
                  borderLeft: `3px solid ${paranoidMode ? 'var(--color-accent)' : 'var(--color-border)'}`,
                  borderRadius: 4,
                  background: paranoidMode
                    ? 'color-mix(in srgb, var(--color-accent) 6%, var(--color-bg-secondary))'
                    : 'var(--color-bg-secondary)',
                  transition: 'background 0.15s, border-color 0.15s',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 12px' }}>
                  <Icon icon="shield" size={13} color={paranoidMode ? 'var(--color-accent)' : 'var(--color-text-muted)'} />
                  <span style={{ flex: 1, fontSize: 12, fontWeight: 600 }}>Paranoid mode</span>
                  <Switch
                    checked={paranoidMode}
                    onChange={(e: React.FormEvent<HTMLInputElement>) => setParanoidMode(e.currentTarget.checked)}
                    style={{ marginBottom: 0 }}
                    large={false}
                    innerLabel="off"
                    innerLabelChecked="on"
                  />
                </div>
                {paranoidMode ? (
                  <div
                    style={{
                      padding: '8px 12px 10px 33px',
                      fontSize: 11,
                      color: 'var(--color-text-secondary)',
                      lineHeight: 1.6,
                      borderTop: '1px solid color-mix(in srgb, var(--color-accent) 20%, transparent)',
                    }}
                  >
                    <div style={{ marginBottom: 3 }}>
                      <strong>Randomize timestamps</strong> — Date modified / accessed rewritten to a random recent date.
                    </div>
                    <div style={{ marginBottom: 3 }}>
                      <strong>Strip Mark of the Web</strong> — Removes Zone.Identifier so recipients can't see where you got the file.
                    </div>
                    <div style={{ fontSize: 10, color: 'var(--color-text-muted)', marginTop: 4, fontStyle: 'italic' }}>
                      These changes can't be undone.
                    </div>
                  </div>
                ) : (
                  <div style={{ padding: '0 12px 8px 33px', fontSize: 11, color: 'var(--color-text-muted)' }}>
                    Randomize timestamps · strip download origin
                  </div>
                )}
              </div>

              {/* Replace originals */}
              <div
                style={{
                  flex: 1,
                  border: `1px solid ${replaceMode ? 'color-mix(in srgb, var(--color-danger) 40%, transparent)' : 'var(--color-border)'}`,
                  borderLeft: `3px solid ${replaceMode ? 'var(--color-danger)' : 'var(--color-border)'}`,
                  borderRadius: 4,
                  background: replaceMode
                    ? 'color-mix(in srgb, var(--color-danger) 6%, var(--color-bg-secondary))'
                    : 'var(--color-bg-secondary)',
                  transition: 'background 0.15s, border-color 0.15s',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 12px' }}>
                  <Icon icon="refresh" size={13} color={replaceMode ? 'var(--color-danger)' : 'var(--color-text-muted)'} />
                  <span style={{ flex: 1, fontSize: 12, fontWeight: 600 }}>Replace originals</span>
                  <Switch
                    checked={replaceMode}
                    onChange={(e: React.FormEvent<HTMLInputElement>) => setReplaceMode(e.currentTarget.checked)}
                    style={{ marginBottom: 0 }}
                    large={false}
                    innerLabel="off"
                    innerLabelChecked="on"
                  />
                </div>
                {replaceMode ? (
                  <div style={{ padding: '8px 12px 10px 33px', fontSize: 11, color: 'var(--color-danger)', lineHeight: 1.5, borderTop: '1px solid color-mix(in srgb, var(--color-danger) 20%, transparent)' }}>
                    Originals will be overwritten in place — no backup is made.
                  </div>
                ) : (
                  <div style={{ padding: '0 12px 8px 33px', fontSize: 11, color: 'var(--color-text-muted)' }}>
                    Saves cleaned copies to _scrubbed/
                  </div>
                )}
              </div>
            </div>

            {error && (
              <div
                style={{
                  marginTop: 10,
                  padding: 10,
                  background: 'var(--color-danger-dim)',
                  border: '1px solid color-mix(in srgb, var(--color-danger) 50%, transparent)',
                  borderRadius: 4,
                  fontSize: 11,
                  color: 'var(--color-danger)',
                }}
              >
                {error}
              </div>
            )}

            {report && (
              <ScrubReportView report={report} roots={selectedPaths} folderPaths={folderPaths} />
            )}
          </>
        )}
      </div>
      <div
        className="wc-dialog-footer"
        style={{
          padding: '12px 24px',
          display: 'flex',
          justifyContent: 'flex-end',
          alignItems: 'center',
          gap: 8,
        }}
      >
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <Button onClick={onClose} minimal>
            Close
          </Button>
          {engineInstalled && selectedPaths.length > 0 && !report && (
            <>
              {!replaceMode && (
                <Button
                  icon="eye-open"
                  onClick={handlePreview}
                  disabled={running}
                  title="Show what would be removed — no files are written"
                >
                  Preview
                </Button>
              )}
              <Button
                intent="primary"
                icon="clean"
                onClick={handleScrub}
                disabled={running}
                loading={running}
              >
                Scrub {selectedPaths.length}{' '}
                {folderPaths.size > 0 ? 'item' : 'file'}
                {selectedPaths.length === 1 ? '' : 's'}
              </Button>
            </>
          )}
          {/* After a dry-run preview, offer "Scrub for real" so the
              operator doesn't have to clear + re-queue. */}
          {report && report.scrubbed.some((r) => r.dryRun) && (
            <Button
              intent="primary"
              icon="clean"
              onClick={handleScrub}
              disabled={running}
              loading={running}
            >
              Scrub for real
            </Button>
          )}
          {scrubOutputDir && (
            <Button
              icon="folder-open"
              onClick={() => invoke('open_path', { path: scrubOutputDir })}
            >
              Open folder
            </Button>
          )}
          {report && (
            <Button onClick={handleClear} icon="reset">
              Clear &amp; scrub more
            </Button>
          )}
        </div>
      </div>
    </Dialog>
  );
}

// ─────────────────────────────────────────────────────────────────────
// ScrubItemProgress — per-queued-item inline progress
// ─────────────────────────────────────────────────────────────────────
//
// One renders on the right of each queued folder/file row while a scrub runs, so
// every item fills ITS OWN "done / total" bar in parallel. The bar is a fixed
// width (uniform across rows) and the count column is fixed width, so all bars
// line up regardless of how long the folder name is. Total comes from the
// engine's up-front plan; a folder with no supported files reads "no files".
function ScrubItemProgress({
  progress,
  allDone,
}: {
  progress?: { done: number; total: number };
  allDone: boolean;
}) {
  const done = progress?.done ?? 0;
  const total = progress?.total ?? 0;
  const complete = allDone || (total > 0 && done >= total);
  const value = total > 0 ? Math.min(1, done / total) : complete ? 1 : 0;
  const label = total > 0 ? `${done} / ${total}` : complete ? 'no files' : '…';
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8, flexShrink: 0 }}>
      <span
        style={{
          fontSize: 10,
          color: complete ? 'var(--color-success)' : 'var(--color-text-muted)',
          fontVariantNumeric: 'tabular-nums',
          fontFamily: 'var(--font-mono)',
          minWidth: 58,
          textAlign: 'right',
        }}
      >
        {label}
      </span>
      <span style={{ width: 190 }}>
        <ProgressBar value={value} intent={complete ? 'success' : 'primary'} />
      </span>
    </span>
  );
}

// ─────────────────────────────────────────────────────────────────────
// ScrubberMissingBanner — direct-install prompt when the dep isn't here
// ─────────────────────────────────────────────────────────────────────
//
// Hidden Data Remover is a ~5 MB one-time download (winget pulls the
// engine binary). We install in-place so the operator never leaves the
// dialog — minimum friction between "I want to share this safely" and
// "scrubbed copy in my downloads."

function ScrubberMissingBanner({
  onInstall,
  installing,
  installError,
}: {
  onInstall: () => void;
  installing: boolean;
  installError: string | null;
}) {
  return (
    <div
      style={{
        padding: 18,
        borderRadius: 8,
        background: 'var(--color-bg-secondary)',
        border: '1px solid var(--color-border)',
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
        alignItems: 'flex-start',
      }}
    >
      <div style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
        <Icon icon="cloud-download" size={20} style={{ color: 'var(--color-accent)' }} />
        <div style={{ fontSize: 14, fontWeight: 700 }}>
          Hidden Data Remover not installed
        </div>
      </div>
      <p
        style={{
          fontSize: 12,
          color: 'var(--color-text-secondary)',
          lineHeight: 1.5,
          margin: 0,
        }}
      >
        Cleaning photos, documents, and videos uses a small engine that runs entirely on your machine — nothing is sent
        anywhere. One-time install (~5 MB). Covers photos from iPhone /
        Android, PDFs, Word / Excel / PowerPoint, video, and RAW camera
        files.
      </p>
      <Button
        intent="primary"
        icon={installing ? undefined : 'download'}
        onClick={onInstall}
        loading={installing}
        disabled={installing}
      >
        {installing ? 'Installing…' : 'Install (~5 MB)'}
      </Button>
      {installError && (
        <div
          style={{
            padding: 8,
            background: 'var(--color-danger-dim)',
            border:
              '1px solid color-mix(in srgb, var(--color-danger) 50%, transparent)',
            borderRadius: 4,
            fontSize: 11,
            color: 'var(--color-danger)',
            width: '100%',
          }}
        >
          {installError}
        </div>
      )}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────
// ScrubReportView — rich post-scrub visualisation
// ─────────────────────────────────────────────────────────────────────
//
// Three layers:
//
//   1. Aggregate banner with "headline numbers" — files scrubbed,
//      bytes saved (with %), category count, IDENTIFYING-DATA warning
//      if anything in the batch was flagged as operator-identifying
//      (GPS / camera serial / author).
//
//   2. Per-category roll-up: a horizontal stacked bar showing what
//      proportion of the stripped bytes belongs to each category,
//      plus chips with category name + total bytes.
//
//   3. Per-file rows: filename, bytes in→out + delta bar, file type
//      tag, and the per-file category chips (each tinted by category).

interface ScrubReportViewProps {
  report: ScrubReport;
  /** The queued roots (folders/files) so the report can break the batch total
   *  back down per folder — mirrors the per-item bars from the run. */
  roots: string[];
  folderPaths: Set<string>;
}

function ScrubReportView({ report, roots, folderPaths }: ScrubReportViewProps) {
  // Per-category roll-up across the whole batch.
  const categoryRollup = (() => {
    const buckets: Record<
      string,
      { category: string; label: string; bytes: number; count: number; isIdentifying: boolean }
    > = {};
    for (const f of report.scrubbed) {
      for (const sf of f.fieldsStripped) {
        const b = buckets[sf.category] ?? {
          category: sf.category,
          label: sf.label,
          bytes: 0,
          count: 0,
          isIdentifying: sf.isIdentifying,
        };
        b.bytes += sf.bytes;
        b.count += 1;
        b.isIdentifying = b.isIdentifying || sf.isIdentifying;
        buckets[sf.category] = b;
      }
    }
    return Object.values(buckets).sort((a, b) => b.bytes - a.bytes);
  })();

  const totalCategoryBytes = categoryRollup.reduce((acc, c) => acc + c.bytes, 0);
  const totalFieldsStripped = report.scrubbed.reduce(
    (acc, f) => acc + f.fieldsStripped.length,
    0,
  );
  const bytesSaved = Math.max(0, report.totalInputBytes - report.totalOutputBytes);
  const savedPct =
    report.totalInputBytes > 0
      ? Math.round((bytesSaved / report.totalInputBytes) * 1000) / 10
      : 0;
  const hasIdentifying = categoryRollup.some((c) => c.isIdentifying);
  const isDryRun = report.scrubbed.some((r) => r.dryRun);
  // Survivors: files where identifying metadata (or a can't-fully-remove
  // advisory) remained after the scrub. This must be surfaced loudly — the
  // backend used to drop survivors silently, so a leaky file read as clean.
  const residualResults = report.scrubbed.filter(
    (r) => (r.residualFields?.length ?? 0) > 0,
  );
  // Collect every file that has GPS coords so we can plot pins on the
  // map. The map ONLY renders when at least one file has coords.
  const gpsResults = report.scrubbed.filter(
    (r): r is ScrubResult & { gpsCoords: GpsCoords } => !!r.gpsCoords,
  );
  // Per-folder breakdown of the batch, so the "N files" total also shows which
  // folder each came from — mirrors the per-item bars. Shown only when it adds
  // information (more than one queued item, or at least one folder).
  const perFolder = roots.map((root) => ({
    root,
    name: basename(root),
    isFolder: folderPaths.has(root),
    cleaned: report.scrubbed.filter((r) => isUnderRoot(r.inputPath, root)).length,
  }));
  const showPerFolder = perFolder.length > 1 || perFolder.some((f) => f.isFolder);

  return (
    <div style={{ marginTop: 14, display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* ── Dry-run banner ────────────────────────────────────────── */}
      {isDryRun && (
        <div
          style={{
            padding: 10,
            background: 'color-mix(in srgb, var(--color-accent) 12%, transparent)',
            border: '1px solid color-mix(in srgb, var(--color-accent) 40%, transparent)',
            borderRadius: 6,
            display: 'flex',
            gap: 8,
            alignItems: 'flex-start',
            fontSize: 12,
          }}
        >
          <Icon icon="eye-open" size={14} style={{ color: 'var(--color-accent)', marginTop: 1 }} />
          <div style={{ flex: 1, color: 'var(--color-text-primary)', lineHeight: 1.5 }}>
            <strong>Preview only.</strong> No files were written. This is
            exactly what would be removed if you click "Scrub for real".
          </div>
        </div>
      )}

      {/* ── Aggregate headline ────────────────────────────────────── */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(110px, 1fr))',
          gap: 8,
        }}
      >
        <StatCard
          label={isDryRun ? 'Inspected' : 'Cleaned'}
          value={`${report.scrubbed.length} file${report.scrubbed.length === 1 ? '' : 's'}`}
        />
        <StatCard
          label={isDryRun ? 'Items to remove' : 'Items removed'}
          value={String(totalFieldsStripped)}
        />
        <StatCard
          label={isDryRun ? 'Size would save' : 'Size saved'}
          value={`${formatBytes(bytesSaved)}`}
          sub={`${savedPct}%`}
        />
        <StatCard label="Types" value={String(categoryRollup.length)} />
      </div>

      {/* ── Per-folder breakdown ──────────────────────────────────── */}
      {showPerFolder && (
        <div>
          <div
            style={{
              fontSize: 10,
              fontWeight: 700,
              textTransform: 'uppercase',
              letterSpacing: 0.8,
              color: 'var(--color-text-muted)',
              marginBottom: 6,
            }}
          >
            {isDryRun ? 'By folder — would clean' : 'By folder — cleaned'}
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            {perFolder.map((f) => (
              <div
                key={f.root}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  padding: '4px 8px',
                  background: 'var(--color-bg-secondary)',
                  border: '1px solid var(--color-border)',
                  borderRadius: 4,
                  fontSize: 11,
                }}
              >
                <Icon
                  icon={f.isFolder ? 'folder-close' : 'document'}
                  size={11}
                  color="var(--color-text-muted)"
                  style={{ flexShrink: 0 }}
                />
                <span
                  title={f.root}
                  style={{
                    flex: 1,
                    minWidth: 0,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                    fontFamily: 'var(--font-mono)',
                  }}
                >
                  {f.name}
                </span>
                <span style={{ flexShrink: 0, color: 'var(--color-text-muted)', fontFamily: 'var(--font-mono)' }}>
                  {f.cleaned} {f.cleaned === 1 ? 'file' : 'files'}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* ── STILL-PRESENT warning (survivors) ─────────────────────── */}
      {!isDryRun && residualResults.length > 0 && (
        <div
          style={{
            padding: 12,
            background: 'var(--color-danger-dim)',
            border: '2px solid var(--color-danger)',
            borderRadius: 6,
            display: 'flex',
            gap: 8,
            alignItems: 'flex-start',
            fontSize: 12,
          }}
        >
          <Icon icon="warning-sign" size={16} style={{ color: 'var(--color-danger)', marginTop: 1, flexShrink: 0 }} />
          <div style={{ flex: 1, color: 'var(--color-danger)', lineHeight: 1.5 }}>
            <strong>
              Not fully clean — {residualResults.length} file
              {residualResults.length === 1 ? '' : 's'} still contain
              {residualResults.length === 1 ? 's' : ''} identifying data.
            </strong>{' '}
            Some metadata could not be removed and may still be recoverable. Do
            NOT treat these as safe to share.
            <ul style={{ margin: '6px 0 0', paddingLeft: 18 }}>
              {residualResults.map((r) => (
                <li key={r.inputPath} style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
                  {basename(r.inputPath)}
                  {' — '}
                  {(r.residualFields ?? []).map((f) => f.label).join('; ')}
                </li>
              ))}
            </ul>
          </div>
        </div>
      )}

      {/* ── Identifying-data callout ──────────────────────────────── */}
      {hasIdentifying && (
        <div
          style={{
            padding: 10,
            background: 'var(--color-danger-dim)',
            border: '1px solid color-mix(in srgb, var(--color-danger) 45%, transparent)',
            borderRadius: 6,
            display: 'flex',
            gap: 8,
            alignItems: 'flex-start',
            fontSize: 12,
          }}
        >
          <Icon icon="shield" size={14} style={{ color: 'var(--color-danger)', marginTop: 1 }} />
          <div style={{ flex: 1, color: 'var(--color-danger)', lineHeight: 1.5 }}>
            <strong>
              {isDryRun
                ? 'Personal info detected.'
                : 'Personal info removed.'}
            </strong>{' '}
            These files contain data that could trace them back to you
            (your camera, location, name, edit dates).
            {isDryRun
              ? ' Will be stripped on real scrub.'
              : residualResults.length > 0
                ? ' Most was stripped — but see the warning above; some files are not fully clean.'
                : ' Now safe to share.'}
          </div>
        </div>
      )}

      {/* ── GPS map (only when coords were found) ─────────────────── */}
      {gpsResults.length > 0 && <GpsCoordList results={gpsResults} isDryRun={isDryRun} />}

      {/* ── Category roll-up ──────────────────────────────────────── */}
      {categoryRollup.length > 0 && (
        <div>
          <div
            style={{
              fontSize: 10,
              fontWeight: 700,
              textTransform: 'uppercase',
              letterSpacing: 0.8,
              color: 'var(--color-text-muted)',
              marginBottom: 6,
            }}
          >
            What we removed
          </div>
          {/* Stacked bar — width proportional to bytes per category. */}
          <div
            style={{
              display: 'flex',
              height: 8,
              borderRadius: 4,
              overflow: 'hidden',
              border: '1px solid var(--color-border)',
              marginBottom: 8,
            }}
          >
            {categoryRollup.map((c) => {
              const t = categoryTint(c.category);
              const widthPct =
                totalCategoryBytes > 0 ? (c.bytes / totalCategoryBytes) * 100 : 0;
              return (
                <div
                  key={c.category}
                  title={`${c.label} — ${formatBytes(c.bytes)} (${c.count} item${c.count === 1 ? '' : 's'})`}
                  style={{
                    width: `${widthPct}%`,
                    background: t.fg,
                    minWidth: widthPct > 0 ? 2 : 0,
                  }}
                />
              );
            })}
          </div>
          <div style={{ display: 'flex', gap: 5, flexWrap: 'wrap' }}>
            {categoryRollup.map((c) => (
              <CategoryChip
                key={c.category}
                category={c.category}
                label={c.label}
                bytes={c.bytes}
                isIdentifying={c.isIdentifying}
                size="md"
              />
            ))}
          </div>
        </div>
      )}

      {/* ── Per-file cards ─────────────────────────────────────────── */}
      {report.scrubbed.length > 0 && (
        <FileCardList results={report.scrubbed} />
      )}

      {/* ── Skipped (unsupported / content-mismatch) — NAMED so the user
             knows exactly which files were left un-cleaned. ─────────── */}
      {!!report.skippedCount && report.skippedCount > 0 && (
        <SkippedFilesNote
          count={report.skippedCount}
          files={report.skippedFiles ?? []}
        />
      )}

      {/* ── Errors (genuine failures, not unsupported-format) ────────── */}
      {report.errors.length > 0 && (
        <ErrorsSummary errors={report.errors} />
      )}
    </div>
  );
}

function SkippedFilesNote({ count, files }: { count: number; files: string[] }) {
  const [expanded, setExpanded] = useState(false);
  // Files matter here — a skipped file is an un-cleaned file. If the backend
  // gave us names, make them expandable; otherwise fall back to the count.
  const hasNames = files.length > 0;
  return (
    <div
      style={{
        border: '1px solid color-mix(in srgb, var(--color-warning, #f59e0b) 40%, transparent)',
        borderRadius: 6,
        overflow: 'hidden',
        fontSize: 11,
      }}
    >
      <button
        onClick={() => hasNames && setExpanded((v) => !v)}
        style={{
          width: '100%',
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '6px 10px',
          background: 'color-mix(in srgb, var(--color-warning, #f59e0b) 8%, var(--color-bg-secondary))',
          border: 'none',
          cursor: hasNames ? 'pointer' : 'default',
          textAlign: 'left',
          color: 'var(--color-text-secondary)',
        }}
      >
        <Icon icon="ban-circle" size={11} style={{ flexShrink: 0, color: 'var(--color-warning, #f59e0b)' }} />
        <span style={{ flex: 1 }}>
          {count} file{count === 1 ? '' : 's'} <strong>not cleaned</strong> — unsupported format or content mismatch
        </span>
        {hasNames && (
          <Icon icon={expanded ? 'chevron-up' : 'chevron-down'} size={12} style={{ color: 'var(--color-text-muted)' }} />
        )}
      </button>
      {expanded && hasNames && (
        <div
          style={{
            maxHeight: 160,
            overflowY: 'auto',
            padding: '6px 10px',
            display: 'flex',
            flexDirection: 'column',
            gap: 3,
            background: 'var(--color-bg-secondary)',
          }}
        >
          {files.map((f) => (
            <div
              key={f}
              title={f}
              style={{
                fontFamily: 'var(--font-mono)',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                color: 'var(--color-text-primary)',
              }}
            >
              {basename(f)}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function ErrorsSummary({ errors }: { errors: ScrubError[] }) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div
      style={{
        border: '1px solid color-mix(in srgb, var(--color-warning, #f59e0b) 50%, transparent)',
        borderRadius: 6,
        overflow: 'hidden',
        fontSize: 12,
      }}
    >
      <button
        onClick={() => setExpanded((v) => !v)}
        style={{
          width: '100%',
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '8px 12px',
          background: 'color-mix(in srgb, var(--color-warning, #f59e0b) 10%, var(--color-bg-secondary))',
          border: 'none',
          cursor: 'pointer',
          textAlign: 'left',
          color: 'var(--color-text-primary)',
        }}
      >
        <Icon icon="warning-sign" size={13} style={{ color: 'var(--color-warning, #f59e0b)', flexShrink: 0 }} />
        <span style={{ flex: 1, fontWeight: 600 }}>
          {errors.length} file{errors.length === 1 ? '' : 's'} failed to process
        </span>
        <Icon icon={expanded ? 'chevron-up' : 'chevron-down'} size={12} style={{ color: 'var(--color-text-muted)' }} />
      </button>
      {expanded && (
        <div
          style={{
            maxHeight: 200,
            overflowY: 'auto',
            padding: '6px 10px',
            display: 'flex',
            flexDirection: 'column',
            gap: 4,
            background: 'var(--color-bg-secondary)',
          }}
        >
          {errors.map((e) => (
            <div key={e.inputPath} style={{ display: 'flex', gap: 8, alignItems: 'flex-start', fontSize: 11 }}>
              <Icon icon="cross-circle" size={11} style={{ color: 'var(--color-danger)', marginTop: 1, flexShrink: 0 }} />
              <div style={{ minWidth: 0 }}>
                <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600 }}>{basename(e.inputPath)}</span>
                <span style={{ color: 'var(--color-text-muted)', marginLeft: 6 }}>{e.message}</span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function StatCard({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div
      style={{
        padding: '8px 10px',
        background: 'var(--color-bg-secondary)',
        border: '1px solid var(--color-border)',
        borderRadius: 4,
      }}
    >
      <div
        style={{
          fontSize: 9,
          fontWeight: 700,
          textTransform: 'uppercase',
          letterSpacing: 0.8,
          color: 'var(--color-text-muted)',
          marginBottom: 2,
        }}
      >
        {label}
      </div>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 6 }}>
        <span
          style={{
            fontSize: 18,
            fontWeight: 700,
            fontFamily: 'var(--font-mono)',
            color: 'var(--color-text-primary)',
            lineHeight: 1,
          }}
        >
          {value}
        </span>
        {sub && (
          <span style={{ fontSize: 11, color: 'var(--color-text-muted)' }}>{sub}</span>
        )}
      </div>
    </div>
  );
}

function FileCard({ result, index }: { result: ScrubResult; index: number }) {
  const bytesDelta = Math.max(0, result.bytesIn - result.bytesOut);
  const metaItems = (result.sampleValues ?? []).map(parseSampleValue).filter((m) => m.value);
  const identifyingFields = result.fieldsStripped.filter((f) => f.isIdentifying);
  const otherFields = result.fieldsStripped.filter((f) => !f.isIdentifying);
  const orderedFields = [...identifyingFields, ...otherFields];
  const residualFields = result.residualFields ?? [];
  const hasResidual = residualFields.length > 0;

  return (
    <div
      style={{
        background: 'var(--color-bg-secondary)',
        border: hasResidual
          ? '2px solid var(--color-danger)'
          : result.gpsCoords
            ? '1px solid color-mix(in srgb, var(--color-danger) 45%, transparent)'
            : '1px solid var(--color-border)',
        borderRadius: 6,
        overflow: 'hidden',
        fontSize: 11,
        flexShrink: 0,
      }}
    >
      {/* ─── Header ─────────────────────────────────────────────── */}
      <div
        style={{
          display: 'flex', alignItems: 'center', gap: 7,
          padding: '6px 10px',
          background: 'var(--color-bg-tertiary)',
          borderBottom: '1px solid var(--color-border)',
        }}
      >
        <Icon
          icon={hasResidual ? 'warning-sign' : 'tick-circle'}
          size={11}
          style={{ color: hasResidual ? 'var(--color-danger)' : 'var(--color-success)', flexShrink: 0 }}
        />
        <span
          style={{
            fontFamily: 'var(--font-mono)', fontWeight: 700, flex: 1, minWidth: 0,
            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
          }}
        >
          Item {String(index + 1).padStart(2, '0')}
        </span>
        {result.gpsCoords && (
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 3, fontSize: 10, color: 'var(--color-danger)', fontWeight: 700, flexShrink: 0 }}>
            <Icon icon="map-marker" size={10} /> GPS
          </span>
        )}
        {bytesDelta > 0 && (
          <span style={{ color: 'var(--color-success)', fontFamily: 'var(--font-mono)', fontSize: 10, flexShrink: 0 }}>
            −{formatBytes(bytesDelta)}
          </span>
        )}
        <Tag minimal style={{ flexShrink: 0, fontSize: 10 }}>{result.fileType}</Tag>
      </div>

      {/* ─── Body ───────────────────────────────────────────────── */}
      <div style={{ display: 'flex' }}>
        <div style={{ flex: 1, minWidth: 0, padding: '9px 10px', display: 'flex', flexDirection: 'column', gap: 8 }}>
          {/* Metadata values — device, author, dates etc. */}
          {metaItems.length > 0 && (
            <div>
              <div style={{ fontSize: 9, fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.9, color: 'var(--color-text-muted)', marginBottom: 5 }}>
                Data found inside:
              </div>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
                {metaItems.map(({ key, value }, i) => (
                  <span
                    key={i}
                    style={{
                      display: 'inline-flex', alignItems: 'center', gap: 4,
                      padding: '3px 8px', borderRadius: 4, fontSize: 11,
                      background: 'var(--color-bg-tertiary)',
                      border: '1px solid var(--color-border)',
                      maxWidth: 280,
                    }}
                  >
                    {key && <span style={{ color: 'var(--color-text-muted)', fontSize: 10, flexShrink: 0 }}>{key}:</span>}
                    <span
                      style={{ fontWeight: 700, color: 'var(--color-text-primary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                      title={value}
                    >
                      {value}
                    </span>
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* GPS coordinate inline */}
          {result.gpsCoords && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <Icon icon="map-marker" size={10} style={{ color: 'var(--color-danger)', flexShrink: 0 }} />
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--color-danger)', fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.4 }}>
                Location data found
              </span>
            </div>
          )}

          {/* Stripped fields — identifying first */}
          {orderedFields.length > 0 && (
            <div>
              <div style={{ fontSize: 9, fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.9, color: 'var(--color-text-muted)', marginBottom: 4 }}>
                {result.dryRun ? 'Will remove:' : 'Removed:'}
              </div>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3 }}>
                {orderedFields.map((f, i) => (
                  <CategoryChip
                    key={`${f.category}-${i}`}
                    category={f.category}
                    label={f.label}
                    bytes={f.bytes}
                    isIdentifying={f.isIdentifying}
                    size="sm"
                  />
                ))}
              </div>
            </div>
          )}

          {/* Still-present metadata — the file is NOT safe to share. */}
          {hasResidual && (
            <div>
              <div style={{ fontSize: 9, fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.9, color: 'var(--color-danger)', marginBottom: 4 }}>
                Still present:
              </div>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3 }}>
                {residualFields.map((f, i) => (
                  <span
                    key={`res-${i}`}
                    title={f.label}
                    style={{
                      display: 'inline-flex', alignItems: 'center', gap: 4,
                      fontSize: 10, padding: '1px 6px', borderRadius: 3,
                      background: 'var(--color-danger-dim)',
                      color: 'var(--color-danger)',
                      border: '1px solid color-mix(in srgb, var(--color-danger) 45%, transparent)',
                      maxWidth: 220, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                    }}
                  >
                    <Icon icon="warning-sign" size={9} style={{ flexShrink: 0 }} />
                    <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{f.label}</span>
                  </span>
                ))}
              </div>
            </div>
          )}

          {metaItems.length === 0 && orderedFields.length === 0 && !result.gpsCoords && !hasResidual && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--color-text-muted)', fontSize: 11, fontStyle: 'italic', padding: '2px 0' }}>
              <Icon icon="tick-circle" size={11} style={{ color: 'var(--color-success)', flexShrink: 0 }} />
              No metadata found — already clean
            </div>
          )}

          {!result.dryRun && result.outputPath && (
            <div style={{ fontSize: 10, color: 'var(--color-text-muted)', fontFamily: 'var(--font-mono)' }}>
              Clean copy written
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function FileCardList({ results }: { results: ScrubResult[] }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {/* Batch file list */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '6px 10px',
          background: 'var(--color-bg-secondary)',
          border: '1px solid var(--color-border)',
          borderRadius: 6,
        }}
      >
        <Icon icon="document" size={11} color="var(--color-text-muted)" style={{ flexShrink: 0 }} />
        <span style={{ flex: 1, minWidth: 0, fontSize: 11, color: 'var(--color-text-primary)', fontWeight: 700 }}>
          File details
        </span>
        <span style={{ fontSize: 11, color: 'var(--color-text-muted)', flexShrink: 0, fontFamily: 'var(--font-mono)' }}>
          {results.length} item{results.length === 1 ? '' : 's'}
        </span>
      </div>

      <div
        className="custom-scrollbar"
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
          gap: 8,
          maxHeight: results.length > 4 ? 560 : undefined,
          overflowY: results.length > 4 ? 'auto' : undefined,
          overflowX: 'hidden',
          overscrollBehavior: 'contain',
          paddingRight: results.length > 4 ? 4 : 0,
          scrollbarGutter: 'stable',
        }}
      >
        {results.map((result, index) => (
          <FileCard key={result.inputPath} result={result} index={index} />
        ))}
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────
// GpsCoordList — Leaflet iframe map only. The file cards stay anonymous.
// ─────────────────────────────────────────────────────────────

interface GpsMapResult {
  inputPath: string;
  gpsCoords: GpsCoords;
}

interface GpsMarkerGroup {
  key: string;
  lat: number;
  lon: number;
  label: string;
  files: string[];
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function buildGpsMarkerGroups(results: GpsMapResult[]): GpsMarkerGroup[] {
  const groups = new Map<string, GpsMarkerGroup>();
  for (const result of results) {
    // Six decimals is roughly 11cm precision and safely groups files that
    // report the same place with tiny floating point differences.
    const key = `${result.gpsCoords.lat.toFixed(6)},${result.gpsCoords.lon.toFixed(6)}`;
    const group = groups.get(key) ?? {
      key,
      lat: result.gpsCoords.lat,
      lon: result.gpsCoords.lon,
      label: result.gpsCoords.label,
      files: [],
    };
    group.files.push(result.inputPath);
    groups.set(key, group);
  }
  return Array.from(groups.values());
}

function GpsCoordList({ results, isDryRun }: { results: GpsMapResult[]; isDryRun: boolean }) {
  if (results.length === 0) return null;

  const groups = buildGpsMarkerGroups(results);
  const markers = groups.map((g) => {
    const popupItems = g.files
      .map((path) => `<li>${escapeHtml(path.split(/[/\\]/).pop() ?? path)}</li>`)
      .join('');
    const popupTitle =
      g.files.length === 1
        ? '1 file at this location'
        : `${g.files.length} files at this location`;
    const popup =
      `<b style="font-size:12px">${popupTitle}</b>` +
      `<br><span style="font-family:monospace;font-size:10px">${escapeHtml(g.label)}</span>` +
      `<ul style="margin:6px 0 0;padding-left:16px;max-height:140px;overflow:auto;font-family:monospace;font-size:10px">${popupItems}</ul>`;
    const tooltip =
      g.files.length === 1
        ? `1 file<br>${escapeHtml(g.label)}`
        : `${g.files.length} files<br>${escapeHtml(g.label)}`;
    return { lat: g.lat, lon: g.lon, radius: g.files.length > 1 ? 11 : 7, tooltip, popup };
  });
  const mapData = {
    markers,
    center: groups.length === 1 ? { lat: groups[0].lat, lon: groups[0].lon } : null,
    bounds: groups.length === 1 ? null : groups.map((g) => [g.lat, g.lon]),
  };
  // Embed as a non-executable JSON data block (CSP does not gate these) and let
  // the bundled, same-origin /leaflet/map-init.js read + render it. Escaping `<`
  // prevents a filename/label containing "</script>" from closing the block.
  const mapDataJson = JSON.stringify(mapData).replace(/</g, '\\u003c');
  const srcdoc = `<!DOCTYPE html><html><head>
<base href="${MAP_ORIGIN}/">
<link rel="stylesheet" href="/leaflet/leaflet.css">
<style>html,body,#m{margin:0;padding:0;height:100%;background:#0a0f12}
.leaflet-popup-content-wrapper{background:#111a1f;color:#e2e8f0;border:1px solid rgba(255,255,255,0.12);border-radius:4px;box-shadow:0 8px 32px rgba(0,0,0,0.5)}
.leaflet-popup-tip{background:#111a1f}
.leaflet-tooltip{background:#111a1f;color:#e2e8f0;border:1px solid rgba(255,255,255,0.14);border-radius:4px;box-shadow:0 6px 20px rgba(0,0,0,0.45);font-family:monospace;font-size:10px;line-height:1.35}
</style>
</head><body><div id="m"></div>
<script type="application/json" id="map-data">${mapDataJson}</script>
<script src="/leaflet/leaflet.js"></script>
<script src="/leaflet/map-init.js"></script>
</body></html>`;

  return (
    <div
      style={{
        border: '1px solid color-mix(in srgb, var(--color-danger) 45%, transparent)',
        borderRadius: 6,
        overflow: 'hidden',
        background: 'var(--color-bg-secondary)',
      }}
    >
      <div
        style={{
          padding: '8px 12px',
          background: 'var(--color-danger-dim)',
          borderBottom: '1px solid color-mix(in srgb, var(--color-danger) 45%, transparent)',
          display: 'flex',
          gap: 8,
          alignItems: 'center',
          fontSize: 12,
        }}
      >
        <Icon icon="map-marker" size={14} style={{ color: 'var(--color-danger)' }} />
        <div style={{ flex: 1, color: 'var(--color-danger)', lineHeight: 1.5 }}>
          <strong>
            Your location was in{' '}
            {results.length === 1 ? 'this file' : `${results.length} files`}.
          </strong>{' '}
          {isDryRun
            ? 'Will be stripped on real scrub.'
            : 'Removed — the cleaned copy has no GPS data.'}
        </div>
      </div>
      {/* Leaflet map — one red pin per file that contained GPS */}
      <div style={{ borderBottom: '1px solid color-mix(in srgb, var(--color-danger) 45%, transparent)' }}>
        <iframe
          srcDoc={srcdoc}
          style={{ width: '100%', height: 200, border: 'none', display: 'block' }}
          title="GPS locations map"
          sandbox="allow-scripts allow-same-origin"
        />
      </div>
    </div>
  );
}
