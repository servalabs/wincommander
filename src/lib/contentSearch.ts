// SPDX-License-Identifier: AGPL-3.0-or-later
// Pure helper functions for the file-content search UI.
// No Tauri IPC here — all functions are pure transforms on plain data.

import type { Chunk, ContentHit, ContentQueryArgs } from "../types/wincmd-search";

export interface SnippetSegment {
  text:        string;
  highlighted: boolean;
}

export interface ContentDisplayRow {
  /** String form of the 64-bit FNV doc id — safe React key, no String() needed. */
  docId:       string;
  path:        string;
  name:        string;
  ext:         string;
  score:       number;
  matchKind:   string;
  snippetHtml: string;
  /** Pre-parsed snippet for accessible rendering. */
  snippetSegs: SnippetSegment[];
  modifiedDisplay: string;
  sizeDisplay:     string;
  author:      string;
  docTitle:    string;
  tags:        string;
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

export function buildContentQueryArgs(
  terms:       string,
  limit?:      number,
  offset?:     number,
  keywordOnly: boolean = true,
): ContentQueryArgs {
  return {
    terms,
    limit:        limit ?? 50,
    offset:       offset ?? 0,
    keyword_only: keywordOnly,
  };
}

// ---------------------------------------------------------------------------
// Snippet rendering
// ---------------------------------------------------------------------------

/**
 * Decode the 5 HTML entities the engine escapes (`to_html()` + the raw-body
 * fallback). Segments render as React text nodes — which re-escape — so the
 * entity-encoded form would otherwise display literally (`Vec&lt;T&gt;`).
 * `&amp;` is decoded last so `&amp;lt;` → `&lt;`, never `<`.
 */
function decodeHtmlEntities(s: string): string {
  return s
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#(?:x27|39);/g, "'")
    .replace(/&amp;/g, "&");
}

/**
 * Parse a tantivy snippet string (HTML-escaped text with <mark>…</mark>
 * highlights) into an array of plain, entity-decoded segments.
 */
export function formatSnippetWithHighlights(snippetHtml: string): SnippetSegment[] {
  const segs: SnippetSegment[] = [];
  // tantivy emits only <mark> tags; body <>&" are escaped, so splitting on
  // <mark> is unambiguous. Decode entities per segment after splitting.
  const parts = snippetHtml.split(/(<mark>.*?<\/mark>)/gs);
  for (const part of parts) {
    if (part.startsWith("<mark>") && part.endsWith("</mark>")) {
      segs.push({
        text:        decodeHtmlEntities(part.slice(6, -7)), // strip <mark>…</mark>
        highlighted: true,
      });
    } else if (part.length > 0) {
      segs.push({ text: decodeHtmlEntities(part), highlighted: false });
    }
  }
  return segs;
}

// ---------------------------------------------------------------------------
// Display formatting
// ---------------------------------------------------------------------------

export function formatContentModified(mtimeUnixSecs: number): string {
  const ms = mtimeUnixSecs * 1000;
  if (!Number.isFinite(ms) || ms === 0) return "—";
  return new Date(ms).toLocaleDateString(undefined, {
    year:  "numeric",
    month: "short",
    day:   "numeric",
  });
}

function formatBytes(bytes: number): string {
  if (bytes < 1024)        return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Reconstruct readable document text from the chunks `content_get_doc` returns.
 * Body chunks reconstruct the extracted body exactly (chunk 0 is the Title);
 * fall back to all chunk text if there are no body chunks (title-only doc).
 */
export function chunksToText(chunks: Chunk[]): string {
  const body = chunks.filter((c) => c.field === "Body").map((c) => c.text).join("");
  if (body.trim().length > 0) return body;
  return chunks.map((c) => c.text).join("\n");
}

export function contentHitToDisplayRow(hit: ContentHit): ContentDisplayRow {
  return {
    docId:           hit.doc_id,
    path:            hit.path,
    name:            hit.name,
    ext:             hit.ext,
    score:           hit.score,
    matchKind:       hit.match_kind,
    snippetHtml:     hit.snippet,
    snippetSegs:     formatSnippetWithHighlights(hit.snippet),
    modifiedDisplay: formatContentModified(hit.mtime),
    sizeDisplay:     formatBytes(hit.size),
    author:          hit.author,
    docTitle:        hit.doc_title,
    tags:            hit.tags,
  };
}

/** True for hits the engine matched only as a file-NAME substring (no token
 *  or body match) — the UI labels these instead of showing a body snippet. */
export function isNameOnlyMatch(row: Pick<ContentDisplayRow, "matchKind">): boolean {
  return row.matchKind === "NameSubstring";
}

/**
 * Drop content hits whose file already appears in the filename-search results,
 * so the merged view never lists the same file twice. Paths are compared
 * case-insensitively (Windows filesystems are case-preserving, not sensitive).
 */
export function dedupeContentRows(
  rows:      ContentDisplayRow[],
  filePaths: Iterable<string>,
): ContentDisplayRow[] {
  const seen = new Set<string>();
  for (const p of filePaths) seen.add(p.toLowerCase());
  if (seen.size === 0) return rows;
  return rows.filter((row) => !seen.has(row.path.toLowerCase()));
}
