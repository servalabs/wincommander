import { Fragment } from 'react';
import { Icon } from '@/components/ui/bp';
import type { RemovedPropertyGroup } from '@/lib/scrubPresentation';

export type Tint = { bg: string; fg: string; border: string };

/** Every removed property name under its group, all visible at once. */
export default function RemovedPropertyGroups({
  groups,
  tintFor,
  valueByDetail,
}: {
  groups: RemovedPropertyGroup[];
  tintFor: (category: string) => Tint;
  valueByDetail?: ReadonlyMap<string, string>;
}) {
  return (
    <dl
      style={{
        display: 'grid', gridTemplateColumns: 'minmax(110px, max-content) minmax(0, 1fr)',
        columnGap: 10, rowGap: 5, alignItems: 'start', margin: 0, fontSize: 11,
      }}
    >
      {groups.map((group) => (
        <Fragment key={group.id}>
          <dt style={{ display: 'flex', alignItems: 'center', gap: 5, color: 'var(--color-text-primary)', fontWeight: 600, lineHeight: '18px' }}>
            {group.isIdentifying && <Icon icon="shield" size={10} style={{ color: 'var(--color-danger)', flexShrink: 0 }} />}
            <span>{group.label}</span>
            <span style={{ color: 'var(--color-text-muted)', fontFamily: 'var(--font-mono)', fontSize: 10, fontWeight: 400 }}>
              {group.properties.length}
            </span>
          </dt>
          <dd style={{ display: 'flex', flexWrap: 'wrap', gap: 3, margin: 0, minWidth: 0 }}>
            {group.properties.map((p) => {
              const t = tintFor(p.category);
              const value = valueByDetail?.get(p.detail);
              return (
                <span
                  key={p.detail}
                  title={`${p.label} — ${p.detail}${value ? `: ${value}` : ''}${p.occurrences > 1 ? ` (${p.occurrences} tags)` : ''}${p.isIdentifying ? ' · could identify you' : ''}`}
                  style={{
                    display: 'inline-flex', alignItems: 'center', gap: 4,
                    fontSize: 10, padding: '1px 6px', borderRadius: 3,
                    background: t.bg, color: t.fg, border: `1px solid ${t.border}`,
                    maxWidth: '100%', overflowWrap: 'anywhere', lineHeight: '16px',
                  }}
                >
                  {p.isIdentifying && <Icon icon="shield" size={9} style={{ flexShrink: 0 }} />}
                  <span>{p.detail}{value && ':'}</span>
                  {value && <strong style={{ color: 'var(--color-text-primary)' }}>{value}</strong>}
                  {p.occurrences > 1 && <span style={{ opacity: 0.7 }}>×{p.occurrences}</span>}
                </span>
              );
            })}
          </dd>
        </Fragment>
      ))}
    </dl>
  );
}
