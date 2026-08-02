// src/panels/search-files/NameResultsSection.tsx
//
// "File names" result group — Everything-backed matches as a slim table.
// Rows are options in the panel's single listbox: click selects,
// double-click opens, and the matched query tokens are highlighted in the
// file name so it's obvious why a row is here. Pure renderer.

import { motion, AnimatePresence } from "framer-motion";
import { Icon } from "@/components/ui/icon";
import { Button } from "@/components/ui/button";
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
}: NameResultsSectionProps) {
  return (
    <div className="sfp-section">
      <div className="sfp-section-bar">
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
      </div>
      {showNoMatches && (
        <div className="sfp-section-empty">
          No file names match <strong>"{query}"</strong>
        </div>
      )}
      {results.length > 0 && (
        <div className="sfp-rows" role="presentation">
          <AnimatePresence initial={false}>
            {results.map((r, i) => {
              const selected = selectedIndex === i;
              return (
                <motion.div
                  key={r.full_path}
                  id={`sfp-opt-${i}`}
                  role="option"
                  aria-selected={selected}
                  className="search-result-row"
                  initial={{ opacity: 0, x: -8 }}
                  animate={{ opacity: 1, x: 0 }}
                  transition={{ delay: Math.min(i * 0.012, 0.3) }}
                  onClick={() => onSelect(i)}
                  onDoubleClick={() => onOpenFile(r.full_path)}
                  title={`${r.full_path} — double-click or press Enter to open`}
                >
                  <div className="sr-col sr-name">
                    <FileIcon path={r.full_path} name={r.name} isDir={isDirectoryResult(r)} iconData={r.icon_data} />
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
                </motion.div>
              );
            })}
          </AnimatePresence>
        </div>
      )}
    </div>
  );
}
