import { useCallback, useState } from 'react';
import { Icon } from '@/components/ui/bp';
import { initiallyOpenFiles } from '@/lib/scrubPresentation';
import type { ScrubResult } from '@/lib/scrubTypes';
import FileDetailSection from './FileDetailSection';
import type { Tint } from './RemovedPropertyGroups';

export const FILE_DETAILS_PAGE_SIZE = 50;

const LINK_BUTTON = {
  display: 'inline-flex', alignItems: 'center', gap: 3, padding: '1px 0', border: 'none', background: 'none',
  color: 'var(--color-accent)', cursor: 'pointer', fontSize: 11, fontWeight: 600, flexShrink: 0,
} as const;

/** Full-width, one-toggle-per-file report. Paged like NotCleanedList so a
 *  batch of hundreds renders only headers it shows, never hidden bodies. */
export default function FileDetailsList({
  results,
  tintFor,
  showTitle = true,
}: {
  results: ScrubResult[];
  tintFor: (category: string) => Tint;
  showTitle?: boolean;
}) {
  const [open, setOpen] = useState<ReadonlySet<string>>(() => initiallyOpenFiles(results));
  const [shown, setShown] = useState(FILE_DETAILS_PAGE_SIZE);
  const toggle = useCallback((path: string) => {
    setOpen((prev) => {
      const next = new Set(prev);
      if (!next.delete(path)) next.add(path);
      return next;
    });
  }, []);
  const isAllOpen = results.every((r) => open.has(r.inputPath));
  const canToggleAll = results.length > 1;
  const remaining = results.length - shown;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6, minWidth: 0 }}>
      {(showTitle || canToggleAll) && (
        <div
          style={{
            display: 'flex', alignItems: 'center', gap: 8, padding: showTitle ? '6px 10px' : '0 2px',
            ...(showTitle ? { background: 'var(--color-bg-secondary)', border: '1px solid var(--color-border)', borderRadius: 6 } : {}),
          }}
        >
          {showTitle && <Icon icon="document" size={11} color="var(--color-text-muted)" style={{ flexShrink: 0 }} />}
          <span style={{ flex: 1, minWidth: 0, fontSize: 11, color: 'var(--color-text-primary)', fontWeight: 700 }}>
            {showTitle && 'File details'}
          </span>
          {showTitle && (
            <span style={{ fontSize: 11, color: 'var(--color-text-muted)', flexShrink: 0, fontFamily: 'var(--font-mono)' }}>
              {results.length} item{results.length === 1 ? '' : 's'}
            </span>
          )}
          {canToggleAll && (
            <button
              type="button"
              onClick={() => setOpen(isAllOpen ? new Set() : new Set(results.map((r) => r.inputPath)))}
              style={LINK_BUTTON}
            >
              <Icon icon={isAllOpen ? 'chevron-up' : 'chevron-down'} size={11} />
              {isAllOpen ? 'Collapse all' : 'Expand all'}
            </button>
          )}
        </div>
      )}

      {results.slice(0, shown).map((result, index) => (
        <FileDetailSection
          key={result.inputPath}
          result={result}
          index={index}
          isOpen={open.has(result.inputPath)}
          onToggle={toggle}
          tintFor={tintFor}
        />
      ))}

      {remaining > 0 && (
        <button type="button" onClick={() => setShown((n) => n + FILE_DETAILS_PAGE_SIZE)} style={{ ...LINK_BUTTON, alignSelf: 'flex-start' }}>
          Show {Math.min(remaining, FILE_DETAILS_PAGE_SIZE)} more ({remaining} not shown)
        </button>
      )}
    </div>
  );
}
