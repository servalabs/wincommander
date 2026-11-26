// Pure presentation rules for the Share Safely (metadata scrubber) dialog:
// file-type icon kind, removed-property grouping, sample values, and engine version line.
import type { ScrubResult, StrippedField } from './scrubTypes';

export type ScrubFileKind =
  | 'images' | 'raw' | 'vector' | 'pdf' | 'word' | 'excel' | 'slides'
  | 'videos' | 'audio' | 'archives' | 'text' | 'generic';

const EXTENSION_KINDS: Record<ScrubFileKind, readonly string[]> = {
  images: ['jpg', 'jpeg', 'jpe', 'png', 'heic', 'heif', 'webp', 'avif', 'tiff', 'tif', 'gif', 'bmp', 'jp2', 'jxl', 'mpo', 'ico', 'psd', 'psb'],
  raw: ['cr2', 'cr3', 'crw', 'nef', 'nrw', 'arw', 'sr2', 'dng', 'rw2', 'orf', 'ori', 'raf', 'pef', 'srw', 'rwl', 'erf', '3fr', 'fff', 'iiq', 'x3f', 'mef', 'mos', 'mrw', 'gpr'],
  vector: ['svg', 'svgz', 'ai', 'eps'],
  pdf: ['pdf'],
  word: ['doc', 'docx', 'docm', 'dot', 'dotx', 'odt', 'rtf'],
  excel: ['xls', 'xlsx', 'xlsm', 'xlt', 'xltx', 'ods', 'csv'],
  slides: ['ppt', 'pptx', 'pptm', 'pot', 'potx', 'odp'],
  videos: ['mp4', 'mov', 'm4v', '3gp', '3g2', 'mkv', 'webm', 'avi', 'wmv', 'mpg', 'mpeg'],
  audio: ['mp3', 'm4a', 'flac', 'wav', 'ogg', 'opus', 'aac', 'wma'],
  archives: ['zip', '7z', 'rar', 'tar', 'gz', 'bz2', 'xz', 'cab', 'iso'],
  text: ['txt', 'md', 'log', 'json', 'xml', 'html', 'htm', 'ps'],
  generic: [],
};

const KIND_BY_EXTENSION = new Map<string, ScrubFileKind>(
  (Object.entries(EXTENSION_KINDS) as [ScrubFileKind, readonly string[]][])
    .flatMap(([kind, exts]) => exts.map((ext) => [ext, kind] as const)),
);

export function fileExtension(path: string): string {
  const name = path.split(/[/\\]/).pop() ?? path;
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

/** Icon kind + short label for a file. The engine's `fileType` wins over the
 *  extension because a renamed file is cleaned as what it really is. */
export function scrubFileKind(path: string, fileType?: string): { kind: ScrubFileKind; label: string } {
  const typed = (fileType ?? '').trim().toLowerCase().replace(/^\./, '');
  const ext = fileExtension(path);
  const kind = KIND_BY_EXTENSION.get(typed) ?? KIND_BY_EXTENSION.get(ext) ?? 'generic';
  const label = (typed && /^[a-z0-9]{1,6}$/.test(typed) ? typed : ext).toUpperCase();
  return { kind, label };
}

export const PROPERTY_GROUPS = [
  { id: 'gps', label: 'GPS / location' },
  { id: 'camera', label: 'Camera & device' },
  { id: 'dates', label: 'Dates & times' },
  { id: 'author', label: 'Author & edits' },
  { id: 'document', label: 'Document properties' },
  { id: 'thumbnail', label: 'Thumbnail' },
  { id: 'color', label: 'Color profile' },
  { id: 'text', label: 'Text comments' },
  { id: 'other', label: 'Other' },
] as const;

export type PropertyGroupId = (typeof PROPERTY_GROUPS)[number]['id'];

const GROUP_IDS = new Set<string>(PROPERTY_GROUPS.map((g) => g.id));

// Order matters: "Exposure Time" is a camera setting, not a date, and a GPS
// timestamp is location data. Paranoid passes are actions, not found data.
const DETAIL_RULES: readonly [PropertyGroupId, RegExp][] = [
  ['gps', /\bgps\b|geotag|latitude|longitude|altitude|coordinates|location|address|\bcity\b|country|sub-?location/],
  ['thumbnail', /thumbnail|preview/],
  ['color', /\bicc\b|colou?r (profile|space)|profile (description|version|class|cmm|copyright)/],
  ['camera', /exposure|shutter|aperture|f ?number|\biso\b|focal|\blens|flash|white balance|metering|serial|\bmake\b|\bmodel\b|camera|device|maker ?note|sensor|scene (type|capture)|brightness|digital zoom|subject distance/],
  ['dates', /date|time|timestamp/],
  ['author', /author|artist|creator|owner|copyright|account|organi[sz]ation|company|manager|by-?line|edit history|history|software|revision|last (saved|modified) by|producer|rights|toolkit|instance id|document id/],
  ['text', /comment|caption|description|keyword|subject|rating|\btext\b|notes?\b/],
  ['document', /document|\btitle\b|template|pages?\b|word count|application|category|hyperlink|custom propert/],
];

const CATEGORY_FALLBACK: Record<string, PropertyGroupId> = {
  exif: 'camera',
  xmp: 'author',
  pdfXmp: 'author',
  iptc: 'text',
  pngText: 'text',
  pngExif: 'camera',
  pdfInfo: 'document',
  officeCore: 'document',
  officeApp: 'document',
  thumbnail: 'thumbnail',
  iccProfile: 'color',
  pngTime: 'dates',
};

/** Maps one removed property name to a user-facing group. An explicit,
 *  known `group` from the engine always wins over the heuristics. */
export function classifyRemovedProperty(category: string, detail: string, group?: string): PropertyGroupId {
  if (group && GROUP_IDS.has(group)) return group as PropertyGroupId;
  if (category === 'paranoid') return 'other';
  if (category === 'thumbnail') return 'thumbnail';
  if (category === 'iccProfile') return 'color';
  const text = detail.toLowerCase().replace(/[_-]+/g, ' ');
  for (const [id, pattern] of DETAIL_RULES) {
    if (pattern.test(text)) return id;
  }
  return CATEGORY_FALLBACK[category] ?? 'other';
}

export interface RemovedProperty {
  detail: string;
  category: string;
  label: string;
  isIdentifying: boolean;
  /** How many tags with this same property name were removed. */
  occurrences: number;
}

export interface RemovedPropertyGroup {
  id: PropertyGroupId;
  label: string;
  isIdentifying: boolean;
  properties: RemovedProperty[];
}

type FieldWithGroup = StrippedField & { group?: string };

/** Groups every removed property name (deduplicated) in PROPERTY_GROUPS order. */
export function groupRemovedProperties(fields: readonly FieldWithGroup[]): RemovedPropertyGroup[] {
  const byGroup = new Map<PropertyGroupId, Map<string, RemovedProperty>>();
  for (const field of fields) {
    const details = field.details?.filter((d) => d.trim()) ?? [];
    for (const detail of details.length > 0 ? details : [field.label]) {
      const id = classifyRemovedProperty(field.category, detail, field.group);
      const bucket = byGroup.get(id) ?? new Map<string, RemovedProperty>();
      const key = detail.trim().toLowerCase();
      const existing = bucket.get(key);
      if (existing) {
        existing.occurrences += 1;
        existing.isIdentifying = existing.isIdentifying || field.isIdentifying;
      } else {
        bucket.set(key, {
          detail: detail.trim(),
          category: field.category,
          label: field.label,
          isIdentifying: field.isIdentifying,
          occurrences: 1,
        });
      }
      byGroup.set(id, bucket);
    }
  }
  return PROPERTY_GROUPS.flatMap(({ id, label }) => {
    const bucket = byGroup.get(id);
    if (!bucket) return [];
    const properties = [...bucket.values()].sort(
      (a, b) => Number(b.isIdentifying) - Number(a.isIdentifying) || a.detail.localeCompare(b.detail),
    );
    return [{ id, label, isIdentifying: properties.some((p) => p.isIdentifying), properties }];
  });
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

export interface SampleValue {
  key: string;
  value: string;
}

export function parseSampleValue(s: string): SampleValue {
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

const nameKey = (name: string) => name.toLowerCase().replace(/[^a-z0-9]/g, '');

/** Attaches each original-file sample to the removed property of the same name;
 *  samples that name no removed property stay as loose examples. */
export function pairSampleValues(
  groups: readonly RemovedPropertyGroup[],
  samples: readonly string[],
): { valueByDetail: Map<string, string>; unmatched: SampleValue[] } {
  const detailByKey = new Map(groups.flatMap((g) => g.properties.map((p) => [nameKey(p.detail), p.detail] as const)));
  const valueByDetail = new Map<string, string>();
  const unmatched: SampleValue[] = [];
  for (const sample of samples.map(parseSampleValue)) {
    if (!sample.value) continue;
    const detail = detailByKey.get(nameKey(sample.key));
    if (sample.key && detail && !valueByDetail.has(detail)) valueByDetail.set(detail, sample.value);
    else unmatched.push(sample);
  }
  return { valueByDetail, unmatched };
}

export const FILE_DETAILS_AUTO_OPEN_MAX = 3;

/** A handful of files all start open; in a big batch only files that still
 *  carry data do, so a not-safe-to-share file is never hidden behind a click. */
export function initiallyOpenFiles(results: readonly ScrubResult[]): Set<string> {
  const isFew = results.length <= FILE_DETAILS_AUTO_OPEN_MAX;
  return new Set(results.filter((r) => isFew || (r.residualFields?.length ?? 0) > 0).map((r) => r.inputPath));
}

export interface EngineVersions {
  exiftool?: string;
  qpdf?: string;
}

const VERSION = /^\d+(?:\.\d+)+$/;

function cleanVersion(value: unknown): string | undefined {
  if (typeof value !== 'string') return undefined;
  const v = value.trim().replace(/^v/i, '');
  return VERSION.test(v) ? v : undefined;
}

/** Extracts engine versions from the dependency probe's free-text version
 *  (e.g. "ExifTool 13.59; qpdf version 12.4.2Run qpdf --copyright …"), preferring
 *  structured values when the engine provides them. */
export function parseEngineVersions(raw: string | null | undefined, structured?: Partial<Record<keyof EngineVersions, unknown>>): EngineVersions {
  const text = raw ?? '';
  const exiftool =
    cleanVersion(structured?.exiftool) ??
    /exiftool\D{0,12}?(\d+(?:\.\d+)+)/i.exec(text)?.[1] ??
    // Older probes reported ExifTool's bare version number before qpdf was bundled.
    /^\s*v?(\d+(?:\.\d+)+)\s*(?:;|$)/.exec(text)?.[1];
  const qpdf = cleanVersion(structured?.qpdf) ?? /qpdf\D{0,12}?(\d+(?:\.\d+)+)/i.exec(text)?.[1];
  return { ...(exiftool ? { exiftool } : {}), ...(qpdf ? { qpdf } : {}) };
}

export const SUPPORTED_FORMATS_LINE = 'JPEG, PNG, HEIC, PDF, Office, RAW, Photoshop, MP4 and more';

export function formatEngineLine(versions: EngineVersions): string {
  return [
    versions.exiftool && `ExifTool ${versions.exiftool}`,
    versions.qpdf && `qpdf ${versions.qpdf}`,
    SUPPORTED_FORMATS_LINE,
  ].filter(Boolean).join(' · ');
}
