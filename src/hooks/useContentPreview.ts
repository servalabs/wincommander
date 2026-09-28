// SPDX-License-Identifier: AGPL-3.0-or-later
// Loads the readable excerpt for the selected content-search result. Kept out
// of the shortcut renderer so result selection remains a pure UI concern.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { chunksToText } from "@/lib/contentSearch";
import type { ContentDisplayRow } from "@/lib/contentSearch";
import type { Chunk } from "@/types/wincmd-search";
import { refreshSearchPrivacy, useSearchPrivacy } from "./useSearchPrivacy";
import { mayShowSearchPath, searchPrivacyLease } from "@/lib/searchPrivacy";

export interface ContentPreview {
  row: ContentDisplayRow | null;
  text: string;
  isLoading: boolean;
  select: (row: ContentDisplayRow) => void;
}

export function useContentPreview(queryKey: string): ContentPreview {
  const privacy = useSearchPrivacy();
  const [rowRevision, setRowRevision] = useState(-1);
  const [row, setRow] = useState<ContentDisplayRow | null>(null);
  const [text, setText] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const requestId = useRef(0);

  // A preview must never outlive the query that produced it. The next content
  // result is selected explicitly when the user opens that tab.
  useEffect(() => {
    requestId.current += 1;
    setRow(null);
    setText("");
  }, [queryKey, privacy.revision]);

  useEffect(() => {
    if (!row) {
      setText("");
      setIsLoading(false);
      return;
    }
    const id = ++requestId.current;
    const lease = searchPrivacyLease();
    const live = () => requestId.current === id && lease() && mayShowSearchPath(row.path);
    setIsLoading(true);
    setText("");
    invoke<Chunk[]>("content_get_doc", { docId: row.docId })
      .then(async (chunks) => {
        await refreshSearchPrivacy(true);
        if (live()) setText(chunksToText(chunks));
      })
      .catch(() => {
        if (live()) setText("");
      })
      .finally(() => {
        if (live()) setIsLoading(false);
      });
    return () => { requestId.current += 1; };
  }, [row]);

  const visible = rowRevision === privacy.revision && row !== null && mayShowSearchPath(row.path);
  return { row: visible ? row : null, text: visible ? text : "", isLoading, select: useCallback((next) => {
    if (next === row && rowRevision === privacy.revision) return;
    requestId.current += 1;
    setText("");
    setRow(next);
    setRowRevision(privacy.revision);
  }, [privacy.revision, row, rowRevision]) };
}
