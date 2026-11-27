// Wire shapes of the `scrub_metadata_paths` report (Share Safely dialog).
// Optional fields are tolerated so the UI keeps working across engine versions.

export interface StrippedField {
  /** Stable category id — drives chip colour + summary roll-up. */
  category: string;
  /** Friendly chip label, e.g. "ICC Color Profile". */
  label: string;
  /** Sanitized names of the metadata properties cleared from this category.
   *  These intentionally contain property names only, never the original
   *  author, device, location, or other personal values. */
  details?: string[];
  /** Bytes of the segment / chunk removed. */
  bytes: number;
  /** True iff the stripped data typically identifies the operator
   *  (GPS, camera serial, author name, dates). Drives red highlight. */
  isIdentifying: boolean;
}

export interface GpsCoords {
  lat: number;
  lon: number;
  /** Human-friendly "37.7749° N, 122.4194° W" string. */
  label: string;
}

export interface ScrubResult {
  inputPath: string;
  outputPath: string;
  fileType: string;
  bytesIn: number;
  bytesOut: number;
  fieldsStripped: StrippedField[];
  /** Present iff the pre-scan extracted valid GPS from the file. */
  gpsCoords?: GpsCoords;
  /** Up to 6 sample values for the per-file tooltip / preview. */
  sampleValues?: string[];
  /** Marker that this row came from a dry-run preview. */
  dryRun?: boolean;
  /** Identifying metadata STILL PRESENT after the scrub (survivors), plus
   *  can't-fully-remove advisories (e.g. a PDF we couldn't rewrite). Non-empty
   *  means this file is NOT safe to share. */
  residualFields?: StrippedField[];
}

export interface ScrubError {
  inputPath: string;
  message: string;
}

export type ScrubOutcome = 'cleaned' | 'already_clean' | 'residual' | 'not_cleaned' | 'failed';

/** One entry per file in the run (engine contract v2). */
export interface ScrubFileOutcome {
  path: string;
  /** The selected path it belongs to, exactly as sent. */
  root: string;
  extension: string;
  detectedType: string | null;
  typeMismatch: boolean;
  cleanedAs: string | null;
  outcome: ScrubOutcome;
  reason: string | null;
  message: string | null;
  outputPath: string | null;
  /** Index into `ScrubReport.scrubbed` for this file's details. */
  scrubbedIndex: number | null;
  removedCategories: string[];
  dryRun: boolean;
}

/** Per selected folder/file totals (engine contract v2). */
export interface ScrubRootSummary {
  root: string;
  isDirectory: boolean;
  exists: boolean;
  total: number;
  cleaned: number;
  alreadyClean: number;
  residual: number;
  notCleaned: number;
  failed: number;
}

export interface ScrubReport {
  scrubbed: ScrubResult[];
  errors: ScrubError[];
  totalInputBytes: number;
  totalOutputBytes: number;
  skippedCount?: number;
  /** Paths of files that were NOT processed (unsupported format / content
   *  mismatch). Named so the user knows exactly what was left un-cleaned. */
  skippedFiles?: string[];
  /** Files that still carry identifying metadata after the scrub. Non-zero
   *  drives the red "still present — not safe to share" warning. */
  residualCount?: number;
  /** Every file in the run with its outcome and reason; absent from older engines. */
  files?: ScrubFileOutcome[];
  /** One summary per selected path, in the order sent; absent from older engines. */
  roots?: ScrubRootSummary[];
  /** Replace mode: the originals kept for Undo (24 hours); absent otherwise. */
  undo?: ScrubUndo;
}

/** Originals of a Replace run that Pro keeps so the run can be undone. */
export interface ScrubUndo {
  jobId: string;
  files: number;
  bytes: number;
  /** When the kept originals are deleted (Unix milliseconds). */
  expiresMs: number;
}

/** Result of `undo_scrub_replace`. */
export interface ScrubUndoReport {
  restored: string[];
  /** Files left as they are (changed since cleaning, ...), with where the original still is. */
  skipped: { path: string; reason: string; keptAt?: string }[];
}
