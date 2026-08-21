// src/panels/search-files/NameResultsSection.tsx
//
// "File names" result group — Everything-backed matches as a slim table.
// Rows are options in the panel's single listbox: click selects,
// double-click opens, and the matched query tokens are highlighted in the
// file name so it's obvious why a row is here. Pure renderer.

import { Icon } from "@/components/ui/icon";
import { Button } from "@/components/ui/button";
import { useSearchResultWindow } from "@/hooks/useSearchResultWindow";
import FileIcon from "./FileIcon";
import { formatResultSize, isDirectoryResult } from "@/lib/fileNameSearch";
import type { SearchResult } from "@/lib/fileNameSearch";
import { highlightName } from "@/lib/searchSelection";

interface NameResultsSectionProps {
  results: SearchResult[];
  query: string;
  isSearching: boolean;
  showNoMatches: boolean;
  engineMissing: boolean;
  selectedIndex: number;
  onSelect: (flatIndex: number) => void;
  onOpenFile: (path: string) => void;
  onOpenFolder: (dir: string) => void;
  onCopyPath: (path: string) => void;
  copiedPath: string | null;
  scrollContainer: HTMLElement | null;
  loadNativeIcons: boolean;
  /** The panel renders the shared tab label. */
  headerless?: boolean;
}

export default function NameResultsSection({
  results,
  query,
  isSearching,
  showNoMatches,
  engineMissing,
  selectedIndex,
  onSelect,
  onOpenFile,
  onOpenFolder,
  onCopyPath,
  copiedPath,
  scrollContainer,
  loadNativeIcons,
  headerless = false,
}: NameResultsSectionProps) {
  const resultWindow = useSearchResultWindow({
    itemCount: results.length,
    rowHeight: 36,
    scrollContainer,
    pinnedIndex: selectedIndex >= 0 && selectedIndex < results.length ? selectedIndex : null,
  });
  const visibleResults = results.slice(resultWindow.window.start, resultWindow.window.end);

  return (
    <div className="sfp-section">
      {!headerless && <div className="sfp-section-bar">
        <span className="sfp-section-label">
          File names
          {!isSearching && results.length > 0 && (
            <span className="sfp-section-count">{results.length.toLocaleString()}</span>
          )}
        </span>
        {engineMissing && (
          <span className="sfp-group-note">
            filename indexer unavailable — showing indexed-name matches under "Inside files"
          </span>
        )}
      </div>}
      {showNoMatches && (
        <div className="sfp-section-empty">
          No file names match <strong>"{query}"</strong>
        </div>
      )}
      {results.length > 0 && (
        <div ref={resultWindow.listRef} className="sfp-rows" role="presentation">
          {resultWindow.beforeHeight > 0 && (
            <div className="sfp-virtual-spacer" style={{ height: resultWindow.beforeHeight }} aria-hidden="true" />
          )}
          {visibleResults.map((r, windowIndex) => {
              const i = resultWindow.window.start + windowIndex;
              const selected = selectedIndex === i;
              return (
                <div
                  key={r.full_path}
                  id={`sfp-opt-${i}`}
                  role="option"
                  aria-selected={selected}
                  className="search-result-row"
                  onClick={() => onSelect(i)}
                  onDoubleClick={() => onOpenFile(r.full_path)}
                  title={`${r.full_path} — double-click or press Enter to open`}
                >
                  <div className="sr-col sr-name">
                    <FileIcon
                      path={r.full_path}
                      name={r.name}
                      isDir={isDirectoryResult(r)}
                      iconData={r.icon_data}
                      loadNativeIcon={loadNativeIcons}
                      priority={selected ? 0 : Math.abs(i - resultWindow.window.viewportCenter) + 1}
                    />
                    <span className="sr-name-text">
                      {/* segments are purely positional; index keys are correct here */}
                      {highlightName(r.name, query).map((seg, si) =>
                        seg.highlighted
                          ? <mark key={`${seg.text}-${si}`}>{seg.text}</mark>
                          : <span key={`${seg.text}-${si}`}>{seg.text}</span>
                      )}
                    </span>
                  </div>
                  <div className="sr-col sr-path" title={r.directory}>
                    <span className="sr-path-text">{r.directory}</span>
                  </div>
                  <div className="sr-col sr-size">
                    {isDirectoryResult(r) ? <span className="sr-dir-tag">DIR</span> : formatResultSize(r.size)}
                  </div>
                  <div className="sr-col sr-date">{r.modified || "—"}</div>
                  <div className="sr-col sr-actions">
                    <Button
                      size="icon"
                      variant="ghost"
                      title="Open file"
                      aria-label={`Open ${r.name}`}
                      onClick={(e) => { e.stopPropagation(); onOpenFile(r.full_path); }}
                    >
                      <Icon icon="document-open" size={14} />
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      title="Open containing folder"
                      aria-label={`Open the containing folder for ${r.name}`}
                      onClick={(e) => { e.stopPropagation(); onOpenFolder(r.directory); }}
                    >
                      <Icon icon="folder-open" size={14} />
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      title="Copy full path"
                      aria-label={`Copy the full path for ${r.name}`}
                      className={copiedPath === r.full_path ? "text-[var(--color-success)]" : undefined}
                      onClick={(e) => { e.stopPropagation(); onCopyPath(r.full_path); }}
                    >
                      <Icon icon={copiedPath === r.full_path ? "tick" : "clipboard"} size={14} />
                    </Button>
                  </div>
                </div>
              );
            })}
          {resultWindow.afterHeight > 0 && (
            <div className="sfp-virtual-spacer" style={{ height: resultWindow.afterHeight }} aria-hidden="true" />
          )}
        </div>
      )}
    </div>
  );
}
