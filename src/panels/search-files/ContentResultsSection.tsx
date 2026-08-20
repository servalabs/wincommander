// src/panels/search-files/ContentResultsSection.tsx
//
// Content-index result group — matches from text extracted out of the folders
// the user chose to index, rendered as two-line rows.
// rows (file line + highlighted snippet line), plus the index status bar,
// indexed-folders management, and the extracted-text expansion pane.
// Rows share the panel's single listbox selection model. Pure renderer.

import { Fragment } from "react";
import { Icon } from "@/components/ui/icon";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import FileIcon from "./FileIcon";
import { IndexedFoldersManager, NoFoldersOnboarding } from "./IndexedFolders";
import { isNameOnlyMatch } from "@/lib/contentSearch";
import type { ContentDisplayRow } from "@/lib/contentSearch";
import type { IndexStatus } from "@/types/wincmd-search";

export function contentRowDir(path: string): string {
  const cut = path.lastIndexOf("\\");
  return cut > 0 ? path.slice(0, cut) : path;
}

interface ContentResultsSectionProps {
  rows: ContentDisplayRow[];
  query: string;
  contentLoading: boolean;
  showNoMatches: boolean;
  indexStatus: IndexStatus | null;
  indexDisplayError: string | null;
  foldersReindexing: boolean;
  showIndexSettings: boolean;
  onToggleIndexSettings: () => void;
  roots: string[];
  reindexing: boolean;
  rescanning: boolean;
  onReindex: () => void;
  onRescan: () => void;
  onAddFolders: () => void;
  onRemoveFolder: (root: string) => void;
  expandedDocId: string | null;
  expandedText: string | null;
  expandedLoading: boolean;
  expandedError: string | null;
  onToggleExpand: (docId: string) => void;
  /** Flat listbox index of this group's first row (= name-result count). */
  flatOffset: number;
  selectedIndex: number;
  onSelect: (flatIndex: number) => void;
  onOpenFile: (path: string) => void;
  onOpenFolder: (dir: string) => void;
  onCopyPath: (path: string) => void;
  copiedPath: string | null;
  loadNativeIcons: boolean;
}

export default function ContentResultsSection(props: ContentResultsSectionProps) {
  const {
    rows, query, contentLoading, showNoMatches,
    indexStatus, indexDisplayError, foldersReindexing,
    showIndexSettings, onToggleIndexSettings,
    roots, reindexing, rescanning, onReindex, onRescan, onAddFolders, onRemoveFolder,
    expandedDocId, expandedText, expandedLoading, expandedError, onToggleExpand,
    flatOffset, selectedIndex, onSelect,
    onOpenFile, onOpenFolder, onCopyPath, copiedPath,
    loadNativeIcons,
  } = props;

  return (
    <div className="sfp-section">
      <div className="sfp-section-bar">
        <span className="sfp-section-label">
          Text inside files
          {!contentLoading && rows.length > 0 && (
            <span className="sfp-section-count">{rows.length.toLocaleString()}</span>
          )}
        </span>
        <div className="sfp-group-header-actions">
          {indexStatus && !indexStatus.is_indexing && indexStatus.indexed_docs > 0 && (
            <span className="sfp-index-status">
              {indexStatus.indexed_docs.toLocaleString()} files indexed
              {indexDisplayError && (
                <span className="sfp-index-status__error"> · {indexDisplayError}</span>
              )}
            </span>
          )}
          <Button
            size="sm"
            variant="ghost"
            aria-pressed={showIndexSettings}
            title="Choose which folders are indexed for inside-file search"
            onClick={onToggleIndexSettings}
          >
            <Icon icon="cog" size={14} />
            Indexed folders
          </Button>
        </div>
      </div>

      {indexStatus?.is_indexing && (
        <div className="sfp-indexing-bar" role="status" aria-live="polite">
          <Spinner size={12} className="sfp-indexing-spinner" />
          <span>
            Indexing… {indexStatus.indexed_docs.toLocaleString()} of{" "}
            {(indexStatus.indexed_docs + indexStatus.pending_docs).toLocaleString()} files
            {foldersReindexing && " · re-indexing…"}
          </span>
          <span className="sfp-indexing-note">Results may be incomplete until done.</span>
          {indexDisplayError && (
            <span className="sfp-index-status__error"> · {indexDisplayError}</span>
          )}
        </div>
      )}

      {/* Folder management — always shown while nothing is indexed, so
          setup stays discoverable. */}
      {(showIndexSettings || roots.length === 0) && (
        <IndexedFoldersManager
          roots={roots}
          reindexing={reindexing}
          rescanning={rescanning}
          onReindex={onReindex}
          onRescan={onRescan}
          onAddFolders={onAddFolders}
          onRemoveFolder={onRemoveFolder}
        />
      )}

      {roots.length === 0 && <NoFoldersOnboarding onAddFolders={onAddFolders} />}

      {contentLoading && <div className="sfp-loading">Searching inside files…</div>}

      {/* search_content is backend-windowed at 50 rows, leaving the other half
          of the 100-row DOM budget for the virtualized filename group. Keep
          content rows whole so expanded extracted-text previews retain their
          measured, variable height. */}
      {!contentLoading && rows.length > 0 && (
        <div className="sfp-rows" role="presentation">
          {rows.map((row, ri) => {
            const flatIndex = flatOffset + ri;
            const selected = selectedIndex === flatIndex;
            const expanded = expandedDocId === row.docId;
            return (
              <Fragment key={row.docId}>
                <div
                  id={`sfp-opt-${flatIndex}`}
                  role="option"
                  aria-selected={selected}
                  className="search-result-row sfp-content-row"
                  title={`${row.path} — double-click or press Enter to open`}
                  onClick={() => onSelect(flatIndex)}
                  onDoubleClick={() => onOpenFile(row.path)}
                >
                  <div className="sfp-content-line1">
                    <FileIcon
                      path={row.path}
                      name={row.name}
                      isDir={false}
                      loadNativeIcon={loadNativeIcons}
                      priority={selected ? 0 : ri + 1}
                    />
                    <span className="sr-name-text">{row.name}</span>
                    <span className="sfp-content-dir" title={row.path}>{contentRowDir(row.path)}</span>
                    <span className="sfp-content-meta">
                      {row.sizeDisplay} · {row.modifiedDisplay}
                    </span>
                    <div className="sr-actions">
                      <Button
                        size="icon"
                        variant="ghost"
                        title={expanded ? "Hide extracted text" : "Preview extracted text"}
                        aria-label={`${expanded ? "Hide extracted text for" : "Preview extracted text for"} ${row.name}`}
                        aria-expanded={expanded}
                        onClick={(e) => { e.stopPropagation(); onToggleExpand(row.docId); }}
                      >
                        <Icon icon={expanded ? "chevron-up" : "chevron-down"} size={14} />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        title="Open file"
                        aria-label={`Open ${row.name}`}
                        onClick={(e) => { e.stopPropagation(); onOpenFile(row.path); }}
                      >
                        <Icon icon="document-open" size={14} />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        title="Open containing folder"
                        aria-label={`Open the containing folder for ${row.name}`}
                        onClick={(e) => { e.stopPropagation(); onOpenFolder(contentRowDir(row.path)); }}
                      >
                        <Icon icon="folder-open" size={14} />
                      </Button>
                      <Button
                        size="icon"
                        variant="ghost"
                        title="Copy full path"
                        aria-label={`Copy the full path for ${row.name}`}
                        className={copiedPath === row.path ? "text-[var(--color-success)]" : undefined}
                        onClick={(e) => { e.stopPropagation(); onCopyPath(row.path); }}
                      >
                        <Icon icon={copiedPath === row.path ? "tick" : "clipboard"} size={14} />
                      </Button>
                    </div>
                  </div>
                  {(row.author || row.tags) && (
                    <div className="sfp-content-authorline">
                      {row.author && <span>by {row.author}</span>}
                      {row.tags && <span>· {row.tags}</span>}
                    </div>
                  )}
                  <div className="sfp-content-snippet">
                    {isNameOnlyMatch(row) && (
                      <span className="sfp-name-match-badge" title="The search text appears in this file's name, not (only) its contents">
                        name match
                      </span>
                    )}
                    <span className="sfp-snippet">
                      {/* Accessible snippet from pre-parsed segments — no dangerouslySetInnerHTML.
                          Segments are purely positional tokens; text can repeat. */}
                      {row.snippetSegs.map((seg, si) =>
                        seg.highlighted
                          ? <mark key={`${seg.text}-${si}`}>{seg.text}</mark>
                          : <span key={`${seg.text}-${si}`}>{seg.text}</span>
                      )}
                    </span>
                  </div>
                </div>
                {expanded && (
                  <div className="sfp-doc-expand">
                    {expandedLoading && (
                      <div className="sfp-doc-expand-loading">
                        <Spinner size={12} />
                        <span>Loading…</span>
                      </div>
                    )}
                    {expandedError && (
                      <div className="sfp-doc-expand-error" role="alert">
                        <Icon icon="warning-sign" size={12} />
                        <span>{expandedError}</span>
                      </div>
                    )}
                    {!expandedLoading && !expandedError && expandedText !== null && (
                      <pre className="sfp-doc-expand-text">{expandedText}</pre>
                    )}
                  </div>
                )}
              </Fragment>
            );
          })}
        </div>
      )}

      {showNoMatches && (
        <div className="sfp-section-empty">
          <p>No matches inside indexed files for <strong>"{query}"</strong></p>
          <p className="search-empty-hint">Try different keywords, or add the folders you need via "Indexed folders".</p>
          <p className="search-empty-hint">
            Tip: refine with ext:pdf, size:&gt;10mb, after:2026-01, author:name
          </p>
        </div>
      )}
    </div>
  );
}
