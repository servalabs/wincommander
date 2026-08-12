// SPDX-License-Identifier: AGPL-3.0-or-later
// Pure translation of the Search Files filter chips (Type/Size/Modified) into
// the query-string filter tokens the content-search backend (filters.rs)
// natively parses out of `search_content`'s `terms` argument. No Tauri IPC —
// see useContentIndex for where the composed string is actually sent.

import { FILE_TYPE_EXTENSIONS } from "@/lib/fileNameSearch";
import type { DateFilter, SearchType, SizeFilter } from "@/lib/fileNameSearch";

/** Zero-padded YYYY-MM-DD using LOCAL date parts — must match the chip's
 *  visible "since" semantics (a user in UTC+5:30 expects "today" to mean
 *  their local midnight, not UTC midnight). */
function formatLocalDate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/**
 * Build the space-joined filter-token string the content backend understands
 * (`ext:`, `size:`, `after:`) from the current Type/Size/Modified chip state.
 * Returns "" when no chips are active. `now` is injectable for deterministic
 * tests; callers should leave it at its default in production.
 */
export function buildContentFilterTokens(
  types: Set<SearchType>,
  size: SizeFilter,
  date: DateFilter,
  now: Date = new Date(),
): string {
  const tokens: string[] = [];

  // Content search only ever matches files (there's no "folder" concept for
  // indexed text), so the files/folders scope chips don't apply here — only
  // the extension categories translate to an ext: token.
  const extCats = Array.from(types).filter((t) => t !== "files" && t !== "folders");
  if (extCats.length > 0) {
    const exts = new Set<string>();
    for (const cat of extCats) {
      for (const ext of FILE_TYPE_EXTENSIONS[cat as keyof typeof FILE_TYPE_EXTENSIONS]) {
        exts.add(ext);
      }
    }
    // Backend expects COMMA separators here (unlike the Everything ext: token
    // in fileNameSearch.ts, which uses semicolons) — see filters.rs.
    tokens.push(`ext:${Array.from(exts).join(",")}`);
  }

  if (size === "tiny") tokens.push("size:<1mb");
  if (size === "medium") tokens.push("size:>=1mb", "size:<=100mb"); // range = two tokens
  if (size === "large") tokens.push("size:>100mb");
  if (size === "huge") tokens.push("size:>1gb");

  if (date === "today") {
    tokens.push(`after:${formatLocalDate(now)}`);
  } else if (date === "week") {
    const d = new Date(now);
    d.setDate(d.getDate() - 7);
    tokens.push(`after:${formatLocalDate(d)}`);
  } else if (date === "month") {
    const d = new Date(now);
    d.setMonth(d.getMonth() - 1);
    tokens.push(`after:${formatLocalDate(d)}`);
  }

  return tokens.join(" ");
}
