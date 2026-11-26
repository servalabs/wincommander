import { memo, useId, useMemo } from 'react';
import { Icon } from '@/components/ui/bp';
import { formatBytes, groupRemovedProperties, pairSampleValues } from '@/lib/scrubPresentation';
import type { ScrubResult } from '@/lib/scrubTypes';
import RemovedPropertyGroups, { type Tint } from './RemovedPropertyGroups';
import ScrubFileIcon from './ScrubFileIcon';
import ScrubInfoButton from './ScrubInfoButton';

const SECTION_LABEL = { display: 'flex', alignItems: 'center', fontSize: 9, fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.9, color: 'var(--color-text-muted)', marginBottom: 5 } as const;

/** One file of the report: a single toggle whose header summarises what goes,
 *  and whose body lists every removed property with no further clicks. */
function FileDetailSection({
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
  const bodyId = useId();
  const removedGroups = useMemo(() => {
    const identifyingFirst = [...result.fieldsStripped.filter((f) => f.isIdentifying), ...result.fieldsStripped.filter((f) => !f.isIdentifying)];
    return groupRemovedProperties(identifyingFirst);
  }, [result.fieldsStripped]);
  const removedCount = removedGroups.reduce((n, g) => n + g.properties.length, 0);
  const bytesDelta = Math.max(0, result.bytesIn - result.bytesOut);
  const residualFields = result.residualFields ?? [];
  const hasResidual = residualFields.length > 0;
  const sourceName = result.inputPath.split(/[/\\]/).pop() || `Item ${String(index + 1).padStart(2, '0')}`;
  const isEmpty = removedGroups.length === 0 && !result.gpsCoords && !hasResidual && (result.sampleValues ?? []).length === 0;

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
        flexShrink: 0,
      }}
    >
      <button
        type="button"
        aria-expanded={isOpen}
        aria-controls={isOpen ? bodyId : undefined}
        onClick={() => onToggle(result.inputPath)}
        title={result.inputPath}
        style={{
          width: '100%', display: 'flex', alignItems: 'flex-start', gap: 7, padding: '6px 10px',
          background: 'var(--color-bg-tertiary)', border: 'none', textAlign: 'left', cursor: 'pointer',
          color: 'var(--color-text-primary)', fontSize: 11,
        }}
      >
        <Icon icon={isOpen ? 'chevron-down' : 'chevron-right'} size={12} style={{ color: 'var(--color-text-muted)', flexShrink: 0, marginTop: 3 }} />
        <span style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 2 }}>
          <span style={{ display: 'flex', alignItems: 'center', gap: 7, minWidth: 0 }}>
            <Icon
              icon={hasResidual ? 'warning-sign' : 'tick-circle'}
              size={11}
              style={{ color: hasResidual ? 'var(--color-danger)' : 'var(--color-success)', flexShrink: 0 }}
            />
            <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 700, flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {sourceName}
            </span>
            {result.gpsCoords && (
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 3, fontSize: 10, color: 'var(--color-danger)', fontWeight: 700, flexShrink: 0 }}>
                <Icon icon="map-marker" size={10} /> GPS
              </span>
            )}
            {removedCount > 0 && (
              <span style={{ fontSize: 10, color: 'var(--color-text-secondary)', fontWeight: 600, flexShrink: 0 }}>
                {removedCount} {result.dryRun ? 'to remove' : 'removed'}
              </span>
            )}
            {bytesDelta > 0 && (
              <span style={{ color: 'var(--color-success)', fontFamily: 'var(--font-mono)', fontSize: 10, flexShrink: 0 }}>
                −{formatBytes(bytesDelta)}
              </span>
            )}
            <ScrubFileIcon path={result.inputPath} fileType={result.fileType} />
          </span>
          <span style={{ color: 'var(--color-text-muted)', fontSize: 10, lineHeight: 1.45, overflowWrap: 'anywhere' }}>
            {hasResidual && (
              <span style={{ color: 'var(--color-danger)', fontWeight: 700 }}>
                {residualFields.length} still present{removedGroups.length > 0 ? ' · ' : ''}
              </span>
            )}
            {removedGroups.length > 0
              ? removedGroups.map((g, i) => (
                <span key={g.id} style={g.isIdentifying ? { color: 'var(--color-danger)' } : undefined}>
                  {i > 0 && <span style={{ color: 'var(--color-text-muted)' }}> · </span>}
                  {g.label} {g.properties.length}
                </span>
              ))
              : !hasResidual && (isEmpty ? 'No metadata found — already clean' : 'Nothing listed to remove')}
          </span>
        </span>
      </button>
      {isOpen && <FileDetailBody id={bodyId} result={result} removedGroups={removedGroups} isEmpty={isEmpty} tintFor={tintFor} />}
    </section>
  );
}

function FileDetailBody({
  id,
  result,
  removedGroups,
  isEmpty,
  tintFor,
}: {
  id: string;
  result: ScrubResult;
  removedGroups: ReturnType<typeof groupRemovedProperties>;
  isEmpty: boolean;
  tintFor: (category: string) => Tint;
}) {
  const { valueByDetail, unmatched } = pairSampleValues(removedGroups, result.sampleValues ?? []);
  const residualFields = result.residualFields ?? [];
  const hasResidual = residualFields.length > 0;
  const outputIsOriginal = result.outputPath === result.inputPath;
  return (
    <div id={id} style={{ padding: '9px 10px', borderTop: '1px solid var(--color-border)', display: 'flex', flexDirection: 'column', gap: 10, minWidth: 0 }}>
      {/* File-by-file receipt; values shown beside a property came from the original pre-scan. */}
      {removedGroups.length > 0 && (
        <div>
          <div style={SECTION_LABEL}>
            {result.dryRun ? 'Will remove from this file:' : 'Removed from this file:'}
            <ScrubInfoButton topic={result.dryRun ? 'what will be removed' : 'what was removed'}>
              {hasResidual
                ? 'Values shown were found before cleaning. Check the remaining-data warning below.'
                : 'Values shown were found before cleaning; the clean copy contains none of them.'}
            </ScrubInfoButton>
          </div>
          <RemovedPropertyGroups groups={removedGroups} tintFor={tintFor} valueByDetail={valueByDetail} />
        </div>
      )}

      {unmatched.length > 0 && (
        <div>
          <div style={SECTION_LABEL}>
            {valueByDetail.size > 0 ? 'Other examples found in the original:' : 'Examples found in the original:'}
            <ScrubInfoButton topic="examples found in the original">
              {hasResidual
                ? 'These values were found before cleaning. Check the remaining-data warning below.'
                : 'These values were present before cleaning and are not in the cleaned copy.'}
            </ScrubInfoButton>
          </div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
            {unmatched.map(({ key, value }, i) => (
              <span
                key={i}
                style={{
                  display: 'inline-flex', alignItems: 'center', gap: 4, padding: '3px 8px', borderRadius: 4, fontSize: 11,
                  background: 'var(--color-bg-tertiary)', border: '1px solid var(--color-border)', maxWidth: '100%', minWidth: 0,
                }}
              >
                {key && <span style={{ color: 'var(--color-text-muted)', fontSize: 10, flexShrink: 0 }}>{key}:</span>}
                <span style={{ fontWeight: 700, color: 'var(--color-text-primary)', overflowWrap: 'anywhere' }}>{value}</span>
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

      {isEmpty && (
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
  );
}

export default memo(FileDetailSection);
