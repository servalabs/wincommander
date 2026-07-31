// SPDX-License-Identifier: AGPL-3.0-or-later
// State + IPC for the filename half of Search Files (Everything-backed
// `search_everything`). Debounced live search, multi-select type filters,
// and the Show-more result-limit ladder. Pure query/filter logic lives in
// src/lib/fileNameSearch.ts.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { buildSearchQuery, sfAppSortScore } from "@/lib/fileNameSearch";
import type { DateFilter, SearchResponse, SearchResult, SearchType, SizeFilter } from "@/lib/fileNameSearch";
import { nextResultLimit, RESULT_LIMIT_LADDER } from "@/lib/searchSelection";

export interface FileSearchState {
  query: string;
  results: SearchResult[];
  isSearching: boolean;
  error: string | null;
  hasSearched: boolean;
  totalCount: number;
  resultLimit: number;
  canShowMore: boolean;
  /** Raw query string the current `results` were fetched for — null until
   *  the first search lands. Rows are STALE while this ≠ `query` (debounce
   *  window); Enter must re-search then, not open a stale row. */
  resultsQuery: string | null;
  searchTypes: Set<SearchType>;
  sizeFilter: SizeFilter;
  dateFilter: DateFilter;
  setQuery: (value: string) => void;
  rerunNow: () => void;
  clear: () => void;
  showMore: () => void;
  toggleSearchType: (t: SearchType) => void;
  clearSearchTypes: () => void;
  setSizeFilter: (s: SizeFilter) => void;
  setDateFilter: (d: DateFilter) => void;
}

export function useFileSearch(): FileSearchState {
  const [query, setQueryState] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hasSearched, setHasSearched] = useState(false);
  const [totalCount, setTotalCount] = useState(0);
  const [resultLimit, setResultLimit] = useState<number>(RESULT_LIMIT_LADDER[0]);
  // Multi-select set. Empty = "All". See SearchType for membership semantics.
  const [searchTypes, setSearchTypes] = useState<Set<SearchType>>(() => new Set());
  const [sizeFilter, setSizeFilterState] = useState<SizeFilter>("any");
  const [dateFilter, setDateFilterState] = useState<DateFilter>("any");
  const [resultsQuery, setResultsQuery] = useState<string | null>(null);

  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Tauri invokes cannot be cancelled once sent.  A broad earlier query can
  // therefore time out after a later, narrower query already succeeded; only
  // the newest run is allowed to update the panel state.
  const runIdRef = useRef(0);
  useEffect(() => () => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
  }, []);

  const performSearch = useCallback(async (
    q: string,
    limit: number,
    types = searchTypes,
    size = sizeFilter,
    date = dateFilter,
  ) => {
    const runId = ++runIdRef.current;
    const isCurrent = () => runIdRef.current === runId;
    const hasActiveFilters = types.size > 0 || size !== "any" || date !== "any";
    const effectiveQuery = q.trim();
    if (!effectiveQuery && !hasActiveFilters) {
      if (!isCurrent()) return;
      setResults([]);
      setHasSearched(false);
      setError(null);
      setResultsQuery(q);
      return;
    }
    if (isCurrent()) {
      setIsSearching(true);
      setError(null);
    }
    try {
      // KT: filters now go to the backend even when a text query is present.
      // The old code sent the bare query, over-fetched 5x, and filtered
      // client-side because "Everything's operators and a free-text query
      // don't compose reliably" — they compose fine; the real cause was
      // `search_everything` passing the whole query as ONE argv entry, which
      // es.exe reads as a quoted phrase, so every multi-token query returned
      // zero rows (see backend.rs::tokenize_es_query). Filtering after a
      // limit meant "folders named assets" could report NO results while
      // thousands existed, because the first 50 rows happened to be files.
      // Client-side re-filtering is deliberately NOT reapplied here: it
      // disagrees with the engine (its "last 7 days from local midnight" vs
      // Everything's `dm:thisweek` = since the start of this week) and would
      // discard rows the backend correctly returned.
      const resp = await invoke<SearchResponse>("search_everything", {
        query: buildSearchQuery(effectiveQuery, types, size, date),
        maxResults: limit,
      });
      const sorted = resp.results.slice().sort((a, b) => {
        const diff = sfAppSortScore(a) - sfAppSortScore(b);
        return diff !== 0 ? diff : a.name.length - b.name.length;
      });
      if (isCurrent()) {
        setResults(sorted);
        setTotalCount(resp.total);
        setHasSearched(true);
        setResultsQuery(q);
      }
    } catch (err) {
      if (isCurrent()) {
        setError(String(err));
        setResults([]);
        setHasSearched(true);
        setResultsQuery(q);
      }
    } finally {
      if (isCurrent()) setIsSearching(false);
    }
  }, [dateFilter, searchTypes, sizeFilter]);

  const setQuery = useCallback((value: string) => {
    // Invalidate in-flight work immediately, not after the debounce delay.
    runIdRef.current += 1;
    setQueryState(value);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => performSearch(value, resultLimit), 300);
  }, [performSearch, resultLimit]);

  const rerunNow = useCallback(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    performSearch(query, resultLimit);
  }, [performSearch, query, resultLimit]);

  const clear = useCallback(() => {
    runIdRef.current += 1;
    if (debounceRef.current) clearTimeout(debounceRef.current);
    setQueryState("");
    setResults([]);
    setHasSearched(false);
    setError(null);
    setResultsQuery("");
  }, []);

  const showMore = useCallback(() => {
    const next = nextResultLimit(resultLimit);
    if (!next) return;
    setResultLimit(next);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    performSearch(query, next);
  }, [performSearch, query, resultLimit]);

  const updateFilters = useCallback((next: Partial<{ types: Set<SearchType>; size: SizeFilter; date: DateFilter }>) => {
    const types = next.types ?? searchTypes;
    const size = next.size ?? sizeFilter;
    const date = next.date ?? dateFilter;
    runIdRef.current += 1;
    if (next.types) setSearchTypes(next.types);
    if (next.size) setSizeFilterState(next.size);
    if (next.date) setDateFilterState(next.date);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    if (query.trim() || types.size > 0 || size !== "any" || date !== "any") {
      debounceRef.current = setTimeout(() => performSearch(query, resultLimit, types, size, date), 120);
    } else {
      setResults([]);
      setHasSearched(false);
      setError(null);
    }
  }, [dateFilter, searchTypes, performSearch, query, resultLimit, sizeFilter]);

  /** Toggle a single SearchType in/out of the multi-select set. Files and
   *  Folders are mutually exclusive (file vs folder scope); the extension
   *  categories combine freely. */
  const toggleSearchType = useCallback((t: SearchType) => {
    const next = new Set(searchTypes);
    if (next.has(t)) {
      next.delete(t);
    } else {
      if (t === "files") next.delete("folders");
      if (t === "folders") next.delete("files");
      next.add(t);
    }
    updateFilters({ types: next });
  }, [searchTypes, updateFilters]);

  const clearSearchTypes = useCallback(() => updateFilters({ types: new Set() }), [updateFilters]);
  const setSizeFilter = useCallback((s: SizeFilter) => updateFilters({ size: s }), [updateFilters]);
  const setDateFilter = useCallback((d: DateFilter) => updateFilters({ date: d }), [updateFilters]);

  return {
    query,
    results,
    isSearching,
    error,
    hasSearched,
    totalCount,
    resultLimit,
    canShowMore: nextResultLimit(resultLimit) !== null && totalCount > resultLimit,
    resultsQuery,
    searchTypes,
    sizeFilter,
    dateFilter,
    setQuery,
    rerunNow,
    clear,
    showMore,
    toggleSearchType,
    clearSearchTypes,
    setSizeFilter,
    setDateFilter,
  };
}
