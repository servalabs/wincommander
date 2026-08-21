// SPDX-License-Identifier: AGPL-3.0-or-later
// Browser coordination for the pure Search Files windowing calculation.

import { useLayoutEffect, useRef, useState } from "react";
import { calculateSearchResultWindow, SEARCH_RESULT_WINDOW_LIMIT } from "@/lib/searchResultWindow";

interface UseSearchResultWindowOptions {
  itemCount: number;
  rowHeight: number;
  scrollContainer: HTMLElement | null;
  pinnedIndex?: number | null;
}

export function useSearchResultWindow({
  itemCount,
  rowHeight,
  scrollContainer,
  pinnedIndex,
}: UseSearchResultWindowOptions) {
  const listRef = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState({ scrollTop: 0, height: 0, listTop: 0 });

  useLayoutEffect(() => {
    if (!scrollContainer) return;
    const syncViewport = () => {
      const list = listRef.current;
      if (!list) return;
      const containerBounds = scrollContainer.getBoundingClientRect();
      const listTop = list.getBoundingClientRect().top - containerBounds.top + scrollContainer.scrollTop;
      setViewport({ scrollTop: scrollContainer.scrollTop, height: scrollContainer.clientHeight, listTop });
    };

    const list = listRef.current;
    if (!list) return;
    syncViewport();
    scrollContainer.addEventListener("scroll", syncViewport, { passive: true });
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(syncViewport);
    observer?.observe(scrollContainer);
    observer?.observe(list);
    return () => {
      scrollContainer.removeEventListener("scroll", syncViewport);
      observer?.disconnect();
    };
  }, [itemCount, scrollContainer]);

  const window = calculateSearchResultWindow({
    itemCount,
    scrollTop: Math.max(0, viewport.scrollTop - viewport.listTop),
    viewportHeight: viewport.height,
    rowHeight,
    pinnedIndex,
  });

  return {
    listRef,
    window,
    beforeHeight: window.start * rowHeight,
    afterHeight: (itemCount - window.end) * rowHeight,
    maxMounted: SEARCH_RESULT_WINDOW_LIMIT,
  };
}
