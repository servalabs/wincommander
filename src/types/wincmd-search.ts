// SPDX-License-Identifier: AGPL-3.0-or-later
// IPC wire-format types for the wincmd-search Tauri commands.
//
// doc_id is a 64-bit FNV-1a hash. Rust serializes it as a JSON string
// (via the u64_str serde module in wincmd-search/src/types.rs) so JS
// never loses precision — JS numbers are f64, safe only up to 2^53.
//
// All field names are snake_case — the Rust types have no serde rename_all,
// so JSON keys are the Rust field names verbatim.

/** Stable 64-bit file identity (FNV-1a hash of canonicalized path), as a string. */
export type DocId = string;

/** How a document matched the search query. */
export type MatchKind = "Keyword" | "Semantic" | "Hybrid" | "NameSubstring";

/** One search result row. */
export interface ContentHit {
  /** FNV-1a hash of the canonicalized path, serialized as a decimal string. */
  doc_id:     DocId;
  path:       string;
  name:       string;
  ext:        string;
  /** File modification time, UNIX seconds. Safe as number (< 2^53). */
  mtime:      number;
  /** File size in bytes. Safe as number (< 2^53 for realistic files). */
  size:       number;
  score:      number;
  match_kind: MatchKind;
  /** HTML-escaped snippet with <mark> highlights. */
  snippet:    string;
  /** Document author, extracted from file metadata. Empty string if absent. */
  author:     string;
  /** Document-internal title (distinct from the file `name`). Empty string if absent. */
  doc_title:  string;
  /** Keywords/subject metadata, as a single string. Empty string if absent. */
  tags:       string;
}

/** Snapshot of indexer progress polled by the UI. */
export interface IndexStatus {
  indexed_docs:     number;
  pending_docs:     number;
  is_indexing:      boolean;
  last_error:       string | null;
  index_size_bytes: number;
}

/** Arguments passed to the search_content Tauri command. */
export interface ContentQueryArgs {
  terms:         string;
  limit?:        number;
  offset?:       number;
  keyword_only?: boolean;
  /** Optional folder to scope the search to — mirrors the filename search's
   *  "in this folder" scope (`search_everything`'s `scope_path`). Absent
   *  means every configured content-search root (today's behaviour,
   *  unchanged). An empty/whitespace-only string is rejected by the backend —
   *  omit the field entirely to mean "no scope". */
  scope_path?:   string;
}

/** Which field a chunk came from. */
export type ChunkField = "Title" | "Body";

/** One indexable text chunk of a document — the shape `content_get_doc`
 *  returns (a `Vec<Chunk>`, NOT a plain string). */
export interface Chunk {
  doc_id:  DocId;
  field:   ChunkField;
  ordinal: number;
  text:    string;
}
