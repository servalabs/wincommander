import { useState, type ReactNode } from 'react';
import { Icon } from '@/components/ui/bp';
import { folderStatusText, type FolderBucket } from '@/lib/scrubReportModel';

/** One queued folder's slice of the report, collapsible behind its status line. */
export default function FolderSection({
  bucket,
  isDryRun,
  defaultExpanded,
  children,
}: {
  bucket: FolderBucket;
  isDryRun: boolean;
  defaultExpanded: boolean;
  children: ReactNode;
}) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  const alarming = !isDryRun && bucket.stillPresent > 0;
  return (
    <section
      aria-label={bucket.name}
      style={{
        border: `1px solid ${alarming ? 'var(--color-danger)' : 'var(--color-border)'}`,
        borderRadius: 6,
        overflow: 'hidden',
        flexShrink: 0,
      }}
    >
      <button
        type="button"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
        title={bucket.root || bucket.name}
        style={{
          width: '100%', display: 'flex', alignItems: 'center', gap: 8, padding: '7px 10px',
          background: 'var(--color-bg-secondary)', border: 'none', textAlign: 'left', cursor: 'pointer',
          color: 'var(--color-text-primary)', fontSize: 12,
        }}
      >
        <Icon icon={expanded ? 'chevron-down' : 'chevron-right'} size={12} style={{ color: 'var(--color-text-muted)', flexShrink: 0 }} />
        <Icon icon={bucket.isFolder ? 'folder-close' : 'document'} size={13} style={{ color: 'var(--color-text-muted)', flexShrink: 0 }} />
        <span style={{ fontWeight: 700, flexShrink: 0, maxWidth: '45%', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {bucket.name}
        </span>
        <span style={{ flex: 1, minWidth: 0, fontSize: 11, color: alarming ? 'var(--color-danger)' : 'var(--color-text-muted)' }}>
          {folderStatusText(bucket, isDryRun)}
        </span>
        <Icon
          icon={alarming ? 'warning-sign' : 'tick-circle'}
          size={12}
          style={{ color: alarming ? 'var(--color-danger)' : 'var(--color-success)', flexShrink: 0 }}
        />
      </button>
      {expanded && (
        <div style={{ padding: 8, borderTop: '1px solid var(--color-border)', display: 'flex', flexDirection: 'column', gap: 8 }}>
          {children}
        </div>
      )}
    </section>
  );
}
