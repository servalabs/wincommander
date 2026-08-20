// src/panels/search-files/index.tsx
//
// SearchFilesPanel — one search box drives BOTH file-name search
// (Everything-backed, everywhere) and inside-file content search
// (wincmd-search index). Results render as two groups in a single
// scroll container with one keyboard-first selection model: focus stays
// in the input; ↑/↓ select, Enter opens, Ctrl+Enter opens the folder,
// Esc clears. State + IPC live in useFileSearch / useContentIndex.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { motion, AnimatePresence } from "framer-motion";
import { useFileSearch } from "@/hooks/useFileSearch";
import { useContentIndex } from "@/hooks/useContentIndex";
import { useSearchHotkey } from "@/hooks/useSearchHotkey";
import { dedupeContentRows } from "@/lib/contentSearch";
import { getIndexDisplayError, getTabFilterSuggestion } from "@/lib/searchFilesPanel";
import { isEngineMissingError } from "@/lib/fileNameSearch";
import { buildContentFilterTokens } from "@/lib/contentQueryFilters";
import { areResultsFresh, buildSelectionEntries, stepSelection } from "@/lib/searchSelection";
import SearchHeader from "./SearchHeader";
import SearchEmptyState from "./SearchEmptyState";
import FilterBar from "./FilterBar";
import NameResultsSection from "./NameResultsSection";
import ContentResultsSection, { contentRowDir } from "./ContentResultsSection";
import "./index.css";

const SEARCH_FILES_HANDOFF_KEY = "wincommander.search-files-query";

export default function SearchFilesPanel() {
  const search = useFileSearch();
  const setFileSearchQuery = search.setQuery;
  // KT: content search previously ignored the Type/Size/Modified chips
  // entirely — compose them into backend query tokens here so "Inside
  // files" results honor the same chips the filename group does.
  const filterTokens = useMemo(
    () => buildContentFilterTokens(search.searchTypes, search.sizeFilter, search.dateFilter),
    [search.searchTypes, search.sizeFilter, search.dateFilter],
  );
  const content = useContentIndex(search.query, filterTokens);
  const hotkey = useSearchHotkey();

  const [copiedPath, setCopiedPath] = useState<string | null>(null);
  // Indexed-folders management is tucked behind a gear toggle now that both
  // result groups share one screen.
  const [showIndexSettings, setShowIndexSettings] = useState(false);
  // Virtual selection across BOTH groups as one flat list; -1 = none.
  // Focus never leaves the input — rows are aria options, not tab stops.
  const [selected, setSelected] = useState(-1);
  const inputRef = useRef<HTMLInputElement>(null);
  const [scrollContainer, setScrollContainer] = useState<HTMLDivElement | null>(null);
  const tabFilter = useMemo(
    () => getTabFilterSuggestion(search.query, search.searchTypes, search.sizeFilter, search.dateFilter),
    [search.query, search.searchTypes, search.sizeFilter, search.dateFilter],
  );

  // Focus input on mount
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const acceptHandoffQuery = useCallback((value: unknown) => {
    if (typeof value !== "string") return;
    const query = value.trim();
    if (!query) return;
    // setQuery schedules the normal file + content searches, so a handoff is
    // a real search rather than merely text prefilled into the input.
    setFileSearchQuery(query);
  }, [setFileSearchQuery]);

  // The compact Ctrl+Space launcher deliberately caps its visible rows. When
  // a query has more matches, it hands its text to this complete results view.
  useEffect(() => {
    const query = window.localStorage.getItem(SEARCH_FILES_HANDOFF_KEY);
    if (query !== null) {
      window.localStorage.removeItem(SEARCH_FILES_HANDOFF_KEY);
      acceptHandoffQuery(query);
    }

    // This also covers a second Ctrl+Space while Search Files is already the
    // active panel (there is no remount in that case), and avoids depending on
    // WebView localStorage sharing between the two native windows.
    const onHandoff = (event: Event) => {
      const detail = (event as CustomEvent<{ query?: unknown }>).detail;
      acceptHandoffQuery(detail?.query);
    };
    window.addEventListener("search-files-query-handoff", onHandoff);
    return () => window.removeEventListener("search-files-query-handoff", onHandoff);
  }, [acceptHandoffQuery]);

  // The same file matched by name AND by content must not list twice —
  // the filename row wins, the content row is dropped.
  const dedupedContentRows = useMemo(
    () => dedupeContentRows(content.contentRows, search.results.map((r) => r.full_path)),
    [content.contentRows, search.results],
  );

  const entries = useMemo(
    () => buildSelectionEntries(search.results.length, dedupedContentRows.length),
    [search.results.length, dedupedContentRows.length],
  );

  // New query text = new list; drop the old selection.
  useEffect(() => { setSelected(-1); }, [search.query]);
  // Clamp a stale selection when the list shrinks under it.
  useEffect(() => {
    setSelected((s) => (s >= entries.length ? (entries.length > 0 ? entries.length - 1 : -1) : s));
  }, [entries.length]);
  // Keep the selected row in view while arrowing.
  useEffect(() => {
    if (selected < 0) return;
    document.getElementById(`sfp-opt-${selected}`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const openFile = useCallback(async (path: string) => {
    try {
      await invoke("open_path", { path });
    } catch {
      // fallback: shell open via run_backend_script pattern
    }
  }, []);

  const openFolder = useCallback(async (dir: string) => {
    try {
      await invoke("open_path", { path: dir });
    } catch { }
  }, []);

  const copyPath = useCallback(async (path: string) => {
    try {
      await navigator.clipboard.writeText(path);
      setCopiedPath(path);
      setTimeout(() => setCopiedPath(null), 1500);
    } catch { }
  }, []);

  const openEntry = useCallback((flatIndex: number, folder: boolean) => {
    const entry = entries[flatIndex];
    if (!entry) return;
    if (entry.kind === "name") {
      const r = search.results[entry.index];
      if (!r) return;
      if (folder) openFolder(r.directory);
      else openFile(r.full_path);
    } else {
      const row = dedupedContentRows[entry.index];
      if (!row) return;
      if (folder) openFolder(contentRowDir(row.path));
      else openFile(row.path);
    }
  }, [entries, search.results, dedupedContentRows, openFile, openFolder]);

  const clearAll = useCallback(() => {
    search.clear();
    content.clearContent();
    setSelected(-1);
  }, [search, content]);

  const acceptTabFilter = useCallback(() => {
    if (!tabFilter) return;
    // Apply the filter before updating the text: setQuery clears the pending
    // filter-triggered debounce and schedules exactly one search without the
    // keyword, so `folder` becomes the Folders filter instead of a name term.
    switch (tabFilter.kind) {
      case "files": case "folders": case "documents": case "images": case "videos":
      case "audio": case "archives": case "apps": case "code":
        search.toggleSearchType(tabFilter.kind);
        break;
      case "big": search.setSizeFilter("large"); break;
      case "small": search.setSizeFilter("tiny"); break;
      case "today": search.setDateFilter("today"); break;
      case "thisWeek": search.setDateFilter("week"); break;
      case "last30Days": search.setDateFilter("month"); break;
      default: return;
    }
    search.setQuery(tabFilter.nextQuery);
    inputRef.current?.focus();
  }, [search, tabFilter]);

  const handleInputKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.altKey && tabFilter) {
      e.preventDefault();
      acceptTabFilter();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      if (entries.length === 0) return;
      e.preventDefault();
      setSelected((s) => stepSelection(s, e.key === "ArrowDown" ? 1 : -1, entries.length));
    } else if (e.key === "Enter") {
      // Enter acts on an EXPLICIT selection immediately (the user picked a
      // visible row). With no selection it opens the first row — but only
      // when the rendered rows belong to the current query text; during the
      // debounce window the rows are still the previous query's, so Enter
      // re-searches instead (old-panel guarantee: Enter never acts on stale
      // results).
      const fresh = areResultsFresh(search.query, search.resultsQuery, content.contentQuery);
      if (selected >= 0) openEntry(selected, e.ctrlKey);
      else if (fresh && entries.length > 0) openEntry(0, e.ctrlKey);
      else search.rerunNow();
    } else if (e.key === "Escape") {
      clearAll();
    }
  }, [entries.length, selected, openEntry, search, content.contentQuery, clearAll, tabFilter, acceptTabFilter]);

  // A missing/stopped filename indexer must not scream over healthy content
  // results — it degrades to an inline notice under the File-names group.
  const engineMissing = isEngineMissingError(search.error);
  const activeError = (engineMissing ? null : search.error) ?? content.contentError;
  const indexDisplayError = getIndexDisplayError(content.indexStatus?.last_error);

  const trimmed = search.query.trim();
  const showContentSection = trimmed.length >= 2 || showIndexSettings || content.currentRoots.length === 0;
  const showNameSection = search.hasSearched || search.isSearching;
  const showEmptyState = !showNameSection && !trimmed && content.currentRoots.length > 0 && !showIndexSettings;
  const anySearching = search.isSearching || content.contentLoading;
  const showFooter = (showNameSection || dedupedContentRows.length > 0) && !activeError;

  return (
    <div className="search-files-panel">
      <SearchHeader hotkey={hotkey} />

      {/* Hero search input — one query drives BOTH result groups */}
      <div className="search-files-input-wrap">
        <div className="search-files-input-group">
          <Icon icon="search" size={16} className="search-files-input-icon" />
          <Input
            ref={inputRef}
            role="combobox"
            aria-expanded={entries.length > 0}
            aria-controls="sfp-results-listbox"
            aria-autocomplete="list"
            aria-activedescendant={selected >= 0 ? `sfp-opt-${selected}` : undefined}
            placeholder="Search file names and inside files…"
            value={search.query}
            onChange={(e) => search.setQuery(e.target.value)}
            onKeyDown={handleInputKeyDown}
            className="search-files-input"
          />
          <div className="search-files-input-right">
            {anySearching ? (
              <Spinner size={16} />
            ) : search.query ? (
              <Button
                variant="ghost"
                size="icon"
                className="search-files-input-clear"
                aria-label="Clear search"
                onClick={clearAll}
              >
                <Icon icon="cross" size={14} />
              </Button>
            ) : null}
          </div>
        </div>
        <div className="search-files-input-hint">
          <span>Matches file names everywhere, and text inside your indexed folders.</span>
          <Tooltip>
            <TooltipTrigger asChild>
              <button type="button" className="sfp-syntax-chip">Search tricks</button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="sfp-syntax-tip">
              <div><code>*.pdf</code> — only that file type</div>
              <div><code>report 2024</code> — names containing both</div>
              <div><code>size:&gt;100mb</code> — big files</div>
              <div><code>dm:today</code> — modified today</div>
              <div><code>regex:pattern</code> — full regex</div>
            </TooltipContent>
          </Tooltip>
        </div>
        {tabFilter && (
          <button type="button" className="sfp-tab-filter" onClick={acceptTabFilter}>
            <Icon icon="filter" size={13} />
            <span>Press <kbd>Tab</kbd> to filter by <strong>{tabFilter.label}</strong></span>
          </button>
        )}
        <FilterBar
          searchTypes={search.searchTypes}
          sizeFilter={search.sizeFilter}
          dateFilter={search.dateFilter}
          onToggleType={search.toggleSearchType}
          onClearTypes={search.clearSearchTypes}
          onSizeChange={search.setSizeFilter}
          onDateChange={search.setDateFilter}
        />
      </div>

      {/* Error (either engine) */}
      <AnimatePresence>
        {activeError && (
          <motion.div
            className="search-error-box"
            initial={{ opacity: 0, y: -8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
          >
            <Icon icon="warning-sign" intent="danger" />
            <div>
              <strong>Search failed</strong>
              <p>{activeError}</p>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Initial empty state — nothing typed, nothing searched */}
      {showEmptyState && <SearchEmptyState indexStatus={content.indexStatus} />}

      {/* One results surface, one scroll, two groups */}
      {!showEmptyState && (showNameSection || showContentSection) && (
        <div className="sfp-results-card">
          <div
            ref={setScrollContainer}
            className="sfp-results-scroll"
            id="sfp-results-listbox"
            role="listbox"
            aria-label="Search results"
          >
            {showNameSection && (
              <NameResultsSection
                results={search.results}
                query={search.query}
                isSearching={search.isSearching}
                showNoMatches={search.hasSearched && !search.error && !search.isSearching && search.results.length === 0}
                engineMissing={engineMissing}
                selectedIndex={selected}
                onSelect={setSelected}
                onOpenFile={openFile}
                onOpenFolder={openFolder}
                onCopyPath={copyPath}
                copiedPath={copiedPath}
                scrollContainer={scrollContainer}
                loadNativeIcons={!search.isSearching && search.resultsQuery === search.query}
              />
            )}
            {showContentSection && (
              <ContentResultsSection
                rows={dedupedContentRows}
                query={search.query}
                contentLoading={content.contentLoading}
                showNoMatches={!content.contentLoading && content.contentRows.length === 0 && trimmed.length >= 2 && !content.contentError && content.currentRoots.length > 0}
                allMatchesDeduped={!content.contentLoading && content.contentRows.length > 0 && dedupedContentRows.length === 0}
                indexStatus={content.indexStatus}
                indexDisplayError={indexDisplayError}
                foldersReindexing={content.foldersReindexing}
                showIndexSettings={showIndexSettings}
                onToggleIndexSettings={() => setShowIndexSettings((v) => !v)}
                roots={content.currentRoots}
                reindexing={content.reindexing}
                rescanning={content.rescanning}
                onReindex={content.reindex}
                onRescan={content.rescan}
                onAddFolders={content.addFolders}
                onRemoveFolder={content.removeFolder}
                expandedDocId={content.expandedDocId}
                expandedText={content.expandedText}
                expandedLoading={content.expandedLoading}
                expandedError={content.expandedError}
                onToggleExpand={content.toggleExpand}
                flatOffset={search.results.length}
                selectedIndex={selected}
                onSelect={setSelected}
                onOpenFile={openFile}
                onOpenFolder={openFolder}
                onCopyPath={copyPath}
                copiedPath={copiedPath}
                loadNativeIcons={!content.contentLoading && content.contentQuery === search.query}
              />
            )}
          </div>
        </div>
      )}

      {/* Footer — plain-language counts + Show more + the keyboard legend */}
      {showFooter && (
        <div className="sfp-footer">
          <div className="sfp-footer-counts">
            {search.hasSearched && !search.error && (
              <span className="result-count">
                {search.totalCount > search.resultLimit
                  ? `Showing ${search.results.length.toLocaleString()} of ${search.totalCount.toLocaleString()} name matches`
                  : `${search.results.length.toLocaleString()} name match${search.results.length !== 1 ? "es" : ""}`}
                {!content.contentLoading && dedupedContentRows.length > 0 &&
                  ` · ${dedupedContentRows.length.toLocaleString()} inside files`}
              </span>
            )}
            {search.canShowMore && (
              <Button size="sm" variant="ghost" onClick={search.showMore} disabled={search.isSearching}>
                Show more
              </Button>
            )}
          </div>
          {entries.length > 0 && (
            <div className="sfp-key-legend" aria-hidden="true">
              <span><kbd>↑</kbd><kbd>↓</kbd> select</span>
              <span><kbd>Enter</kbd> open</span>
              <span><kbd>Ctrl</kbd>+<kbd>Enter</kbd> open folder</span>
              <span><kbd>Esc</kbd> clear</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
