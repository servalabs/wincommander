import { memo, useId, useMemo } from 'react';
import { Icon } from '@/components/ui/bp';
import { formatBytes, groupRemovedProperties, parseSampleValue } from '@/lib/scrubPresentation';
import type { ScrubResult } from '@/lib/scrubTypes';
import RemovedPropertyGroups, { type Tint } from './RemovedPropertyGroups';
import ScrubFileIcon from './ScrubFileIcon';
import ScrubInfoButton from './ScrubInfoButton';

const SECTION_LABEL = { display: 'flex', alignItems: 'center', fontSize: 9, fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.9, color: 'var(--color-text-muted)', marginBottom: 5 } as const;

/** One file of the report as a grid card: examples always show, and the whole
 *  removal receipt sits behind one toggle whose label summarises it. */
function FileCard({
  result,
  index,
  isOpen,
  onToggle,
  tintFor,
}: {
  result: ScrubResult;
  index: number;
  isOpen: boolean;
  onToggle: (inputPath: string) => void;
  tintFor: (category: string) => Tint;
}) {
  const removedId = useId();
  const removedGroups = useMemo(() => {
    const identifyingFirst = [...result.fieldsStripped.filter((f) => f.isIdentifying), ...result.fieldsStripped.filter((f) => !f.isIdentifying)];
    return groupRemovedProperties(identifyingFirst);
  }, [result.fieldsStripped]);
  const examples = useMemo(() => (result.sampleValues ?? []).map(parseSampleValue).filter((m) => m.value), [result.sampleValues]);
  const removedCount = removedGroups.reduce((n, g) => n + g.properties.length, 0);
  const bytesDelta = Math.max(0, result.bytesIn - result.bytesOut);
  const residualFields = result.residualFields ?? [];
  const hasResidual = residualFields.length > 0;
  const sourceName = result.inputPath.split(/[/\\]/).pop() || `Item ${String(index + 1).padStart(2, '0')}`;
  const outputIsOriginal = result.outputPath === result.inputPath;

  return (
    <section
      aria-label={sourceName}
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
        minWidth: 0,
      }}
    >
      <div
        style={{
          display: 'flex', alignItems: 'center', gap: 7, padding: '6px 10px',
          background: 'var(--color-bg-tertiary)', borderBottom: '1px solid var(--color-border)',
        }}
      >
        <Icon
          icon={hasResidual ? 'warning-sign' : 'tick-circle'}
          size={11}
          style={{ color: hasResidual ? 'var(--color-danger)' : 'var(--color-success)', flexShrink: 0 }}
        />
        <span
          title={result.inputPath}
          style={{ fontFamily: 'var(--font-mono)', fontWeight: 700, flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
        >
          {sourceName}
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
        <ScrubFileIcon path={result.inputPath} fileType={result.fileType} />
      </div>

      <div style={{ minWidth: 0, padding: '9px 10px', display: 'flex', flexDirection: 'column', gap: 8 }}>
        {/* Values below came from the original pre-scan, not the clean copy. */}
        {examples.length > 0 && (
          <div>
            <div style={SECTION_LABEL}>
              Examples found in the original:
              <ScrubInfoButton topic="examples found in the original">
                {hasResidual
                  ? 'These values were found before cleaning. Check the remaining-data warning below.'
                  : 'These values were present before cleaning and are not in the cleaned copy.'}
              </ScrubInfoButton>
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
              {examples.map(({ key, value }, i) => (
                <span
                  key={i}
                  style={{
                    display: 'inline-flex', alignItems: 'center', gap: 4, padding: '3px 8px', borderRadius: 4, fontSize: 11,
                    background: 'var(--color-bg-tertiary)', border: '1px solid var(--color-border)', maxWidth: '100%', minWidth: 0,
                  }}
                >
                  {key && <span style={{ color: 'var(--color-text-muted)', fontSize: 10, flexShrink: 0 }}>{key}:</span>}
                  <span title={value} style={{ fontWeight: 700, color: 'var(--color-text-primary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {value}
                  </span>
                </span>
              ))}
            </div>
          </div>
        )}

        {result.gpsCoords && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <Icon icon="map-marker" size={10} style={{ color: 'var(--color-danger)', flexShrink: 0 }} />
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--color-danger)', fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.4 }}>
              {result.dryRun
                ? 'Location to remove:'
                : hasResidual
                  ? 'Location found before cleaning — check remaining-data warning:'
                  : 'Location removed:'}{' '}
              {result.gpsCoords.label}
            </span>
          </div>
        )}

        {removedGroups.length > 0 && (
          <div>
            <div style={SECTION_LABEL}>
              {result.dryRun ? 'Will remove from this file:' : 'Removed from this file:'}
              <ScrubInfoButton topic={result.dryRun ? 'what will be removed' : 'what was removed'}>
                {hasResidual
                  ? 'This receipt lists property names. Check the remaining-data warning below.'
                  : 'This receipt lists property names; the clean copy contains none of their original values.'}
              </ScrubInfoButton>
            </div>
            <button
              type="button"
              aria-expanded={isOpen}
              aria-controls={isOpen ? removedId : undefined}
              onClick={() => onToggle(result.inputPath)}
              style={{
                width: '100%', display: 'flex', alignItems: 'flex-start', gap: 6, padding: '4px 6px',
                background: 'var(--color-bg-tertiary)', border: '1px solid var(--color-border)', borderRadius: 4,
                textAlign: 'left', cursor: 'pointer', color: 'var(--color-text-primary)', fontSize: 11,
              }}
            >
              <Icon icon={isOpen ? 'chevron-down' : 'chevron-right'} size={11} style={{ color: 'var(--color-text-muted)', flexShrink: 0, marginTop: 2 }} />
              <span style={{ flex: 1, minWidth: 0, lineHeight: 1.45, overflowWrap: 'anywhere' }}>
                <strong>{removedCount} item{removedCount === 1 ? '' : 's'}</strong>
                <span style={{ fontSize: 10, color: 'var(--color-text-muted)' }}>
                  {removedGroups.map((g) => (
                    <span key={g.id}>
                      {' · '}
                      <span style={g.isIdentifying ? { color: 'var(--color-danger)' } : undefined}>{g.label} {g.properties.length}</span>
                    </span>
                  ))}
                </span>
              </span>
            </button>
            {isOpen && (
              <div id={removedId} style={{ marginTop: 6, minWidth: 0 }}>
                <RemovedPropertyGroups groups={removedGroups} tintFor={tintFor} />
              </div>
            )}
          </div>
        )}

        {/* Still-present metadata — the file is NOT safe to share. */}
        {hasResidual && (
          <div>
            <div style={{ ...SECTION_LABEL, color: 'var(--color-danger)', marginBottom: 4 }}>Still present:</div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3 }}>
              {residualFields.map((f, i) => (
                <span
                  key={`res-${i}`}
                  title={f.label}
                  style={{
                    display: 'inline-flex', alignItems: 'center', gap: 4, fontSize: 10, padding: '1px 6px', borderRadius: 3,
                    background: 'var(--color-danger-dim)', color: 'var(--color-danger)',
                    border: '1px solid color-mix(in srgb, var(--color-danger) 45%, transparent)', maxWidth: '100%', overflowWrap: 'anywhere',
                  }}
                >
                  <Icon icon="warning-sign" size={9} style={{ flexShrink: 0 }} />
                  <span>{f.label}</span>
                </span>
              ))}
            </div>
          </div>
        )}

        {examples.length === 0 && removedGroups.length === 0 && !result.gpsCoords && !hasResidual && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--color-text-muted)', fontSize: 11, fontStyle: 'italic', padding: '2px 0' }}>
            <Icon icon="tick-circle" size={11} style={{ color: 'var(--color-success)', flexShrink: 0 }} />
            No metadata found — already clean
          </div>
        )}

        {!result.dryRun && result.outputPath && (
          <div style={{ minWidth: 0, fontSize: 10, color: 'var(--color-text-muted)' }}>
            <div style={{ marginBottom: 2 }}>{outputIsOriginal ? 'Scrubbed original saved at:' : 'Clean copy saved at:'}</div>
            <div title={result.outputPath} style={{ fontFamily: 'var(--font-mono)', color: 'var(--color-text-primary)', overflowWrap: 'anywhere', lineHeight: 1.4 }}>
              {result.outputPath}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}

export default memo(FileCard);
