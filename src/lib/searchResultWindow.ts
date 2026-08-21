// SPDX-License-Identifier: AGPL-3.0-or-later
// Pure windowing math for the Search Files result groups. Keeping it outside
// React makes the mounted-row budget easy to test without a browser harness.

export const SEARCH_RESULT_WINDOW_LIMIT = 48;

export interface SearchResultWindow {
  start: number;
  end: number;
  viewportCenter: number;
}

interface SearchResultWindowOptions {
  itemCount: number;
  scrollTop: number;
  viewportHeight: number;
  rowHeight: number;
  pinnedIndex?: number | null;
  maxMounted?: number;
  overscan?: number;
}

/**
 * Returns the half-open range that should stay mounted for one result group.
 * A selected row is pinned into the window so the combobox's aria-activedescendant
 * always identifies a real option while keyboard navigation crosses long lists.
 */
export function calculateSearchResultWindow({
  itemCount,
  scrollTop,
  viewportHeight,
  rowHeight,
  pinnedIndex = null,
  maxMounted = SEARCH_RESULT_WINDOW_LIMIT,
  overscan = 8,
}: SearchResultWindowOptions): SearchResultWindow {
  if (itemCount <= 0 || maxMounted <= 0 || rowHeight <= 0) {
    return { start: 0, end: 0, viewportCenter: 0 };
  }

  const mounted = Math.min(itemCount, maxMounted);
  const firstVisible = Math.max(0, Math.floor(scrollTop / rowHeight));
  const visibleRows = Math.max(1, Math.ceil(viewportHeight / rowHeight));
  const viewportCenter = Math.min(itemCount - 1, firstVisible + Math.floor(visibleRows / 2));
  let start = Math.max(0, Math.min(firstVisible - overscan, itemCount - mounted));

  if (pinnedIndex !== null && pinnedIndex >= 0 && pinnedIndex < itemCount) {
    if (pinnedIndex < start) start = pinnedIndex;
    if (pinnedIndex >= start + mounted) start = pinnedIndex - mounted + 1;
    start = Math.max(0, Math.min(start, itemCount - mounted));
  }

  return { start, end: start + mounted, viewportCenter };
}
