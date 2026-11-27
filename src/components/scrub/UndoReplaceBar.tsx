// SPDX-License-Identifier: AGPL-3.0-or-later
// "Undo" for a Replace-originals run: Pro keeps every replaced original for
// 24 hours; this bar puts them back. Files changed after cleaning are left
// alone and listed with where their original still is.
import { useState } from 'react';
import { Button, Icon } from '@/components/ui/bp';
import { formatBytes } from '@/lib/scrubPresentation';
import type { ScrubUndo, ScrubUndoReport } from '@/lib/scrubTypes';

export function undoExpiryLabel(expiresMs: number, now: number = Date.now()): string {
  const hours = Math.max(0, Math.round((expiresMs - now) / 3_600_000));
  if (expiresMs <= now) return 'Undo has expired';
  return hours >= 1 ? `Undo is available for about ${hours} more hour${hours === 1 ? '' : 's'}` : 'Undo expires within the hour';
}

interface Props {
  undo: ScrubUndo;
  /** Runs `undo_scrub_replace`; injected so the bar renders without Tauri in tests. */
  onUndo: (jobId: string) => Promise<ScrubUndoReport>;
  /** Shown instead of the button once the run was undone (lifted so it survives re-renders). */
  initialResult?: ScrubUndoReport | null;
}

export default function UndoReplaceBar({ undo, onUndo, initialResult = null }: Props) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ScrubUndoReport | null>(initialResult);
  const [error, setError] = useState<string | null>(null);
  const files = `${undo.files} original${undo.files === 1 ? '' : 's'}`;
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setResult(await onUndo(undo.jobId));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div
      data-undo-bar
      style={{
        marginTop: 10,
        padding: '8px 12px',
        border: '1px solid var(--color-border)',
        borderRadius: 4,
        background: 'var(--color-bg-secondary)',
        fontSize: 11,
        lineHeight: 1.5,
      }}
    >
      {result ? (
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontWeight: 600 }}>
            <Icon icon="undo" size={12} />
            {result.restored.length} of {undo.files} original{undo.files === 1 ? '' : 's'} put back
          </div>
          {result.skipped.map((s) => (
            <div key={s.path} style={{ color: 'var(--color-warning, #f59e0b)', overflowWrap: 'anywhere' }}>
              {s.path}: {s.reason}
              {s.keptAt ? ` (the original is kept at ${s.keptAt} until Undo expires)` : ''}
            </div>
          ))}
        </div>
      ) : (
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <Icon icon="history" size={12} color="var(--color-text-muted)" />
          <span style={{ flex: 1 }}>
            {files} ({formatBytes(undo.bytes)}) kept. {undoExpiryLabel(undo.expiresMs)}.
          </span>
          <Button small icon="undo" loading={busy} onClick={run}>
            Undo
          </Button>
        </div>
      )}
      {error && <div style={{ color: 'var(--color-danger)', marginTop: 4 }}>{error}</div>}
    </div>
  );
}
