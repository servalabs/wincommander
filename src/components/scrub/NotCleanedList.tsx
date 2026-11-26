import { useState } from 'react';
import { Icon } from '@/components/ui/bp';
import { displayFolder, groupNotCleaned, isNeutralReason, skipReasonLabel, type NotCleanedEntry, type NotCleanedGroup } from '@/lib/scrubReportModel';
import FileTypeIcon from '../FileTypeIcon';
import ScrubFileIcon from './ScrubFileIcon';

const WARN = 'var(--color-warning, #f59e0b)';
export const NOT_CLEANED_PAGE_SIZE = 50;

function NotCleanedRow({ entry, roots }: { entry: NotCleanedEntry; roots: readonly string[] }) {
  const name = entry.path.split(/[/\\]/).pop() ?? entry.path;
  const folder = displayFolder(entry.path, roots);
  return (
    <div
      title={entry.path}
      style={{ display: 'grid', gridTemplateColumns: 'auto minmax(0, 1.2fr) minmax(0, 1fr)', alignItems: 'center', columnGap: 8, padding: '2px 0' }}
    >
      {entry.isFolder ? <FileTypeIcon kind="folders" size={16} /> : <ScrubFileIcon path={entry.path} size={16} showLabel={false} />}
      <span style={{ minWidth: 0, fontFamily: 'var(--font-mono)', color: 'var(--color-text-primary)', overflowWrap: 'anywhere' }}>
        {name}
        {entry.note && <span style={{ fontFamily: 'inherit', color: WARN, marginLeft: 6 }}>{entry.note}</span>}
      </span>
      <span style={{ minWidth: 0, color: 'var(--color-text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        {folder}
      </span>
    </div>
  );
}

// Paged instead of a nested scroll box: a scroll region inside the dialog's
// scrolling body painted empty in WebView2 once it held hundreds of rows.
function ReasonGroup({ group, roots }: { group: NotCleanedGroup; roots: readonly string[] }) {
  const [shown, setShown] = useState(NOT_CLEANED_PAGE_SIZE);
  const remaining = group.entries.length - shown;
  return (
    <div data-reason={group.reason}>
      <div style={{ fontWeight: 700, color: 'var(--color-text-secondary)', margin: '4px 0 2px' }}>
        {group.label} <span style={{ fontWeight: 400, color: 'var(--color-text-muted)' }}>· {group.entries.length}</span>
      </div>
      {group.entries.slice(0, shown).map((entry) => (
        <NotCleanedRow key={entry.path} entry={entry} roots={roots} />
      ))}
      {remaining > 0 && (
        <button
          type="button"
          onClick={() => setShown((n) => n + NOT_CLEANED_PAGE_SIZE)}
          style={{ marginTop: 2, padding: '1px 0', border: 'none', background: 'none', color: 'var(--color-accent)', cursor: 'pointer', fontSize: 11 }}
        >
          Show {Math.min(remaining, NOT_CLEANED_PAGE_SIZE)} more ({remaining} not shown)
        </button>
      )}
    </div>
  );
}

/** Every file the engine left un-cleaned, grouped by the reason it gave. Open by
 *  default: the names are the point, and the old collapsed panel hid them. */
export default function NotCleanedList({
  count,
  entries,
  roots,
  defaultExpanded = true,
}: {
  count: number;
  entries: NotCleanedEntry[];
  roots: readonly string[];
  defaultExpanded?: boolean;
}) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  const groups = groupNotCleaned(entries);
  const neutral = entries.filter((e) => isNeutralReason(e.reason)).length;
  const total = Math.max(count, entries.length);
  const problems = total - neutral;
  const unnamed = total - entries.length;
  const problemGroups = groups.filter((g) => !isNeutralReason(g.reason));
  const summaryParts =
    problemGroups.length === 0 ? (problems > 0 ? [skipReasonLabel('unreported').toLowerCase()] : [])
      : problemGroups.length === 1 && neutral === 0 ? [problemGroups[0].label.toLowerCase()]
        : problemGroups.map((g) => `${g.entries.length} ${g.label.toLowerCase()}`);
  if (problems > 0 && neutral > 0) summaryParts.push(`${neutral} nothing to remove`);
  const tone = problems > 0 ? WARN : 'var(--color-success)';
  const canExpand = entries.length > 0;
  return (
    <div style={{ border: `1px solid color-mix(in srgb, ${tone} 40%, transparent)`, borderRadius: 6, overflow: 'hidden', fontSize: 11 }}>
      <button
        type="button"
        aria-expanded={canExpand ? expanded : undefined}
        onClick={() => canExpand && setExpanded((v) => !v)}
        style={{
          width: '100%', display: 'flex', alignItems: 'center', gap: 6, padding: '6px 10px',
          background: `color-mix(in srgb, ${tone} 8%, var(--color-bg-secondary))`, border: 'none',
          cursor: canExpand ? 'pointer' : 'default', textAlign: 'left', color: 'var(--color-text-secondary)',
        }}
      >
        <Icon icon={problems > 0 ? 'ban-circle' : 'tick-circle'} size={11} style={{ flexShrink: 0, color: tone }} />
        <span style={{ flex: 1, minWidth: 0 }}>
          {problems > 0 ? (
            <>{problems} file{problems === 1 ? '' : 's'} <strong>not cleaned</strong></>
          ) : (
            <>{neutral} file{neutral === 1 ? '' : 's'} with <strong>nothing to remove</strong></>
          )}
          {summaryParts.length > 0 && <>{' — '}{summaryParts.join(' · ')}</>}
        </span>
        {canExpand && (
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 3, flexShrink: 0, color: 'var(--color-accent)', fontWeight: 600 }}>
            {expanded ? 'Hide list' : `Show ${entries.length} file${entries.length === 1 ? '' : 's'}`}
            <Icon icon={expanded ? 'chevron-up' : 'chevron-down'} size={12} />
          </span>
        )}
      </button>
      {expanded && canExpand && (
        <div style={{ padding: '4px 10px 8px', display: 'flex', flexDirection: 'column', gap: 4, background: 'var(--color-bg-secondary)' }}>
          {groups.map((group) => (
            <ReasonGroup key={group.reason} group={group} roots={roots} />
          ))}
          {unnamed > 0 && (
            <div style={{ color: 'var(--color-text-muted)', fontStyle: 'italic' }}>
              {unnamed} more file{unnamed === 1 ? ' was' : 's were'} not cleaned but not named by the engine.
            </div>
          )}
        </div>
      )}
    </div>
  );
}
