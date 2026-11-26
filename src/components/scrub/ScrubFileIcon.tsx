import FileTypeIcon from '../FileTypeIcon';
import { scrubFileKind, type ScrubFileKind } from '@/lib/scrubPresentation';

function RawGlyph({ size }: { size: number }) {
  return (
    <svg className="esb-file-type-icon" data-kind="raw" width={size} height={size} viewBox="0 0 20 20" aria-hidden="true" focusable="false">
      <rect x="1.6" y="4.6" width="16.8" height="12.2" rx="2" fill="#2F6F8F" />
      <path fill="#2F6F8F" d="M6.4 4.9 7.7 2.9h4.6l1.3 2z" />
      <circle cx="10" cy="10.7" r="3.6" fill="#fff" />
      <circle cx="10" cy="10.7" r="2.1" fill="#1F4E66" />
      <rect x="14.6" y="6.4" width="2" height="1.2" rx="0.4" fill="#F6E27A" />
    </svg>
  );
}

function VectorGlyph({ size }: { size: number }) {
  return (
    <svg className="esb-file-type-icon" data-kind="vector" width={size} height={size} viewBox="0 0 20 20" aria-hidden="true" focusable="false">
      <rect x="1.6" y="1.6" width="16.8" height="16.8" rx="3" fill="#C8642A" />
      <path d="M4.6 14.6C6 7.4 14 12.6 15.4 5.4" fill="none" stroke="#fff" strokeWidth="1.5" strokeLinecap="round" />
      <rect x="3.6" y="13.6" width="2.2" height="2.2" fill="#fff" />
      <rect x="14.3" y="4.3" width="2.2" height="2.2" fill="#fff" />
    </svg>
  );
}

function Glyph({ kind, size }: { kind: ScrubFileKind; size: number }) {
  if (kind === 'raw') return <RawGlyph size={size} />;
  if (kind === 'vector') return <VectorGlyph size={size} />;
  // FileTypeIcon draws its generic document for kinds it doesn't know.
  return <FileTypeIcon kind={kind === 'generic' ? 'files' : kind} size={size} />;
}

/** File-type icon with the extension as a tiny caption (and tooltip). */
export default function ScrubFileIcon({
  path,
  fileType,
  size = 18,
  showLabel = true,
}: {
  path: string;
  fileType?: string;
  size?: number;
  showLabel?: boolean;
}) {
  const { kind, label } = scrubFileKind(path, fileType);
  return (
    <span
      title={label ? `${label} file` : 'File'}
      data-scrub-kind={kind}
      style={{ display: 'inline-flex', alignItems: 'center', gap: 3, flexShrink: 0 }}
    >
      <Glyph kind={kind} size={size} />
      {showLabel && label && (
        <span style={{ fontSize: 9, fontWeight: 600, letterSpacing: 0.3, color: 'var(--color-text-muted)', fontFamily: 'var(--font-mono)' }}>
          {label}
        </span>
      )}
    </span>
  );
}
