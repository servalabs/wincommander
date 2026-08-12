// SPDX-License-Identifier: AGPL-3.0-or-later
// Loads the readable excerpt for the selected content-search result. Kept out
// of the shortcut renderer so result selection remains a pure UI concern.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { chunksToText } from "@/lib/contentSearch";
import type { ContentDisplayRow } from "@/lib/contentSearch";
import type { Chunk } from "@/types/wincmd-search";

export interface ContentPreview {
  row: ContentDisplayRow | null;
  text: string;
  isLoading: boolean;
  select: (row: ContentDisplayRow) => void;
}

export function useContentPreview(queryKey: string): ContentPreview {
  const [row, setRow] = useState<ContentDisplayRow | null>(null);
  const [text, setText] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const requestId = useRef(0);

  // A preview must never outlive the query that produced it. The next content
  // result is selected explicitly when the user opens that tab.
  useEffect(() => {
    requestId.current += 1;
    setRow(null);
  }, [queryKey]);

  useEffect(() => {
    if (!row) {
      setText("");
      setIsLoading(false);
      return;
    }
    const id = ++requestId.current;
    setIsLoading(true);
    setText("");
    invoke<Chunk[]>("content_get_doc", { docId: row.docId })
      .then((chunks) => {
        if (requestId.current === id) setText(chunksToText(chunks));
      })
      .catch(() => {
        // The result snippet remains useful while an index is warming up.
        if (requestId.current === id) setText(row.snippetSegs.map((segment) => segment.text).join(""));
      })
      .finally(() => {
        if (requestId.current === id) setIsLoading(false);
      });
  }, [row]);

  return { row, text, isLoading, select: useCallback((next) => setRow(next), []) };
}
