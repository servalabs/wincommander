import { useState } from 'react';
import { Icon } from '@/components/ui/bp';
import type { RemovedPropertyGroup } from '@/lib/scrubPresentation';

type Tint = { bg: string; fg: string; border: string };

/** Removed property names grouped by kind; every group starts collapsed to its name and count. */
export default function RemovedPropertyGroups({
  groups,
  tintFor,
}: {
  groups: RemovedPropertyGroup[];
  tintFor: (category: string) => Tint;
}) {
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const toggle = (id: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      {groups.map((group) => {
        const isOpen = open.has(group.id);
        return (
          <div key={group.id}>
            <button
              type="button"
              aria-expanded={isOpen}
              onClick={() => toggle(group.id)}
              style={{
                display: 'flex', alignItems: 'center', gap: 5, width: '100%',
                padding: '2px 4px', border: 'none', borderRadius: 3, background: 'transparent',
                color: 'var(--color-text-primary)', cursor: 'pointer', textAlign: 'left', fontSize: 11,
              }}
            >
              <Icon icon={isOpen ? 'chevron-down' : 'chevron-right'} size={11} style={{ color: 'var(--color-text-muted)', flexShrink: 0 }} />
              {group.isIdentifying && <Icon icon="shield" size={10} style={{ color: 'var(--color-danger)', flexShrink: 0 }} />}
              <span style={{ fontWeight: 600 }}>{group.label}</span>
              <span style={{ color: 'var(--color-text-muted)', fontFamily: 'var(--font-mono)', fontSize: 10 }}>
                {group.properties.length}
              </span>
            </button>
            {isOpen && (
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3, padding: '3px 0 5px 20px' }}>
                {group.properties.map((p) => {
                  const t = tintFor(p.category);
                  return (
                    <span
                      key={p.detail}
                      title={`${p.label} — ${p.detail}${p.occurrences > 1 ? ` (${p.occurrences} tags)` : ''}${p.isIdentifying ? ' · could identify you' : ''}`}
                      style={{
                        display: 'inline-flex', alignItems: 'center', gap: 4,
                        fontSize: 10, padding: '1px 6px', borderRadius: 3,
                        background: t.bg, color: t.fg, border: `1px solid ${t.border}`,
                        maxWidth: '100%', overflowWrap: 'anywhere',
                      }}
                    >
                      {p.isIdentifying && <Icon icon="shield" size={9} style={{ flexShrink: 0 }} />}
                      <span>{p.detail}</span>
                      {p.occurrences > 1 && <span style={{ opacity: 0.7 }}>×{p.occurrences}</span>}
                    </span>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
