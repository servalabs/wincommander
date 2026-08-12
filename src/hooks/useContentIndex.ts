// SPDX-License-Identifier: AGPL-3.0-or-later
// State + IPC for the content half of Search Files (`wincmd-search` engine):
// debounced live content search, index-status polling, indexed-roots
// management, and per-row extracted-text expansion. Pure display transforms
// live in src/lib/contentSearch.ts.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { useAppState } from "../context/AppContext";
import { buildContentQueryArgs, chunksToText, contentHitToDisplayRow } from "@/lib/contentSearch";
import { mergeIndexedRoots, removeIndexedRoot } from "@/lib/searchFilesPanel";
import type { ContentDisplayRow } from "@/lib/contentSearch";
import type { Chunk, ContentHit, IndexStatus } from "@/types/wincmd-search";

export interface ContentIndexState {
  contentRows: ContentDisplayRow[];
  contentLoading: boolean;
  contentError: string | null;
  /** Raw query string the current `contentRows` were fetched for — null
   *  until the first content search lands (rows are stale while ≠ query). */
  contentQuery: string | null;
  clearContent: () => void;
  indexStatus: IndexStatus | null;
  currentRoots: string[];
  foldersReindexing: boolean;
  reindexing: boolean;
  rescanning: boolean;
  addFolders: () => Promise<void>;
  removeFolder: (root: string) => Promise<void>;
  reindex: () => Promise<void>;
  rescan: () => Promise<void>;
  expandedDocId: string | null;
  expandedText: string | null;
  expandedLoading: boolean;
  expandedError: string | null;
  toggleExpand: (docId: string) => Promise<void>;
}

/**
 * `query` is the panel's single search box — content search follows it live.
 * `filterTokens` is the precomputed `ext:`/`size:`/`after:` token string built
 * from the Type/Size/Modified chips (see contentQueryFilters.ts) — the caller
 * owns memoizing it so this hook stays a plain consumer of both.
 */
export function useContentIndex(query: string, filterTokens: string): ContentIndexState {
  const { appSettings, refreshSettings } = useAppState();

  const [contentRows, setContentRows] = useState<ContentDisplayRow[]>([]);
  const [contentLoading, setContentLoading] = useState(false);
  const [contentError, setContentError] = useState<string | null>(null);
  const [contentQuery, setContentQuery] = useState<string | null>(null);
  const [indexStatus, setIndexStatus] = useState<IndexStatus | null>(null);
  const [foldersReindexing, setFoldersReindexing] = useState(false);
  const [reindexing, setReindexing] = useState(false);
  const [rescanning, setRescanning] = useState(false);
  // Expand/collapse full doc text per content hit row (keyed by docId string).
  const [expandedDocId, setExpandedDocId] = useState<string | null>(null);
  const [expandedText, setExpandedText] = useState<string | null>(null);
  const [expandedLoading, setExpandedLoading] = useState(false);
  const [expandedError, setExpandedError] = useState<string | null>(null);

  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // ── Indexed folders management ────────────────────────────────────────
  const [optimisticRoots, setOptimisticRoots] = useState<string[] | null>(null);
  const settingsRoots = useMemo<string[]>(
    () => appSettings?.app?.fileSearch?.roots ?? [],
    [appSettings?.app?.fileSearch?.roots]
  );
  const currentExclusions = useMemo<string[]>(
    () => appSettings?.app?.fileSearch?.exclusions ?? [],
    [appSettings?.app?.fileSearch?.exclusions]
  );
  const currentRoots = optimisticRoots ?? settingsRoots;

  useEffect(() => {
    if (!optimisticRoots) return;
    if (JSON.stringify(optimisticRoots) === JSON.stringify(settingsRoots)) {
      setOptimisticRoots(null);
    }
  }, [optimisticRoots, settingsRoots]);

  const addFolders = useCallback(async () => {
    const selected = await openFolderPicker({ directory: true, multiple: true });
    if (!selected) return;
    const added = Array.isArray(selected) ? selected : [selected];
    if (added.length === 0) return;
    const merged = mergeIndexedRoots(currentRoots, added);
    setOptimisticRoots(merged);
    setFoldersReindexing(true);
    try {
      await invoke("content_index_configure", { roots: merged, exclusions: currentExclusions });
      await refreshSettings();
    } catch {
      setOptimisticRoots(settingsRoots);
    } finally {
      setFoldersReindexing(false);
    }
  }, [currentRoots, currentExclusions, refreshSettings, settingsRoots]);

  const removeFolder = useCallback(async (root: string) => {
    const next = removeIndexedRoot(currentRoots, root);
    setOptimisticRoots(next);
    setFoldersReindexing(true);
    try {
      await invoke("content_index_configure", { roots: next, exclusions: currentExclusions });
      await refreshSettings();
    } catch {
      setOptimisticRoots(settingsRoots);
    } finally {
      setFoldersReindexing(false);
    }
  }, [currentRoots, currentExclusions, refreshSettings, settingsRoots]);

  const reindex = useCallback(async () => {
    setReindexing(true);
    try {
      await invoke("content_reindex");
    } catch { /* errors surfaced via status poller */ } finally {
      setReindexing(false);
    }
  }, []);

  // Incremental rescan: re-crawl the roots to pick up new/missed/changed files
  // WITHOUT wiping the index, so results stay searchable throughout. The
  // background crawl continues after the invoke resolves; the status poller
  // reflects its progress via `is_indexing`.
  const rescan = useCallback(async () => {
    setRescanning(true);
    try {
      await invoke("content_rescan");
    } catch { /* errors surfaced via status poller */ } finally {
      setRescanning(false);
    }
  }, []);

  // Toggle expand/collapse of full extracted text for a content hit.
  // docId is always a string (64-bit FNV hash serialised as decimal).
  const toggleExpand = useCallback(async (docId: string) => {
    if (expandedDocId === docId) {
      // Collapse
      setExpandedDocId(null);
      setExpandedText(null);
      setExpandedError(null);
      return;
    }
    setExpandedDocId(docId);
    setExpandedText(null);
    setExpandedError(null);
    setExpandedLoading(true);
    try {
      // content_get_doc returns Vec<Chunk> (NOT a string) — join the body
      // chunks into readable text for the <pre> preview.
      const chunks = await invoke<Chunk[]>("content_get_doc", { docId });
      setExpandedText(chunksToText(chunks));
    } catch (e) {
      setExpandedError(String(e));
    } finally {
      setExpandedLoading(false);
    }
  }, [expandedDocId]);

  // Poll content index status. Cheap by design: fast (3s) only while
  // indexing is actually in progress; once idle/complete, slow down to a
  // 15s heartbeat instead of hammering the backend.
  useEffect(() => {
    let id: ReturnType<typeof setTimeout> | null = null;
    let cancelled = false;
    const poll = async () => {
      try {
        const s = await invoke<IndexStatus>("content_index_status");
        if (cancelled) return;
        setIndexStatus(s);
        id = setTimeout(poll, s.is_indexing ? 3000 : 15000);
      } catch {
        /* not ready yet — suppress, retry at the fast interval */
        if (!cancelled) id = setTimeout(poll, 3000);
      }
    };
    poll();
    return () => {
      cancelled = true;
      if (id) clearTimeout(id);
    };
  }, []);

  // On mount, nudge the backend to seed default roots
  // (Desktop/Downloads/Documents on first run) and pull them into appSettings,
  // so the folder list + onboarding reflect reality instead of flashing
  // "no folders indexed" while the defaults are already being indexed.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await invoke("content_index_status"); // triggers backend ensure_initialized
        if (!cancelled) await refreshSettings();
      } catch { /* ignore — the status poller will retry */ }
    })();
    return () => { cancelled = true; };
  }, [refreshSettings]);

  // Live content search follows the shared query. Debounced (~275ms) and
  // gated at 2+ chars so it doesn't fire invoke() on every keystroke;
  // contentRows is only cleared when the query is actually emptied, not on
  // every intermediate keystroke.
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    const trimmed = query.trim();
    if (!trimmed) { setContentRows([]); setContentLoading(false); setContentError(null); setContentQuery(query); return; }
    // KT: content search still needs a TEXT term even when filter chips are
    // set — "PDFs modified this week" with an empty box stays empty here on
    // purpose. The filename group already covers browse-by-filter with no
    // text, and running content search on chips alone would mean sending the
    // backend an arbitrary match-all query.
    if (trimmed.length < 2) return;
    setContentLoading(true);
    setContentError(null);
    // Chip state rides along as extra query-string tokens (ext:/size:/after:)
    // the backend's filters.rs already parses out of `terms` — see
    // contentQueryFilters.ts for the mapping.
    const terms = [trimmed, filterTokens].filter(Boolean).join(" ");
    debounceRef.current = setTimeout(() => {
      invoke<ContentHit[]>("search_content", buildContentQueryArgs(terms) as unknown as Record<string, unknown>)
        .then((hits) => { setContentRows(hits.map(contentHitToDisplayRow)); setContentQuery(query); })
        // On failure drop the previous query's rows too — keeping them would
        // let Enter open a stale row while the error box says "Search failed".
        .catch((e) => { setContentError(String(e)); setContentRows([]); setContentQuery(query); })
        .finally(() => setContentLoading(false));
    }, 275);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
    // KT: filterTokens is a dependency so toggling a chip while the text is
    // unchanged re-runs the search — without it, chip changes would silently
    // never reach the backend until the text query also changed.
  }, [query, filterTokens]);

  const clearContent = useCallback(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    setContentRows([]);
    setContentLoading(false);
    setContentError(null);
    setContentQuery("");
    setExpandedDocId(null);
    setExpandedText(null);
    setExpandedError(null);
  }, []);

  return {
    contentRows,
    contentLoading,
    contentError,
    contentQuery,
    clearContent,
    indexStatus,
    currentRoots,
    foldersReindexing,
    reindexing,
    rescanning,
    addFolders,
    removeFolder,
    reindex,
    rescan,
    expandedDocId,
    expandedText,
    expandedLoading,
    expandedError,
    toggleExpand,
  };
}
