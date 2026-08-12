// src/panels/search-files/SearchEmptyState.tsx
//
// Initial empty state — the searching-detective illustration shown before
// anything is typed. Pure renderer.

import type { IndexStatus } from "@/types/wincmd-search";

export default function SearchEmptyState({ indexStatus }: { indexStatus: IndexStatus | null }) {
  return (
    <div className="search-empty-state">
      <div className="search-detective" aria-hidden="true">
        <div className="search-detective-head" />
        <div className="search-detective-hat" />
        <div className="search-detective-body" />
        <div className="search-detective-glass" />
        <div className="search-detective-beam" />
      </div>
      <p>Just type — file names everywhere <em>and</em> the text inside your files, in one search.</p>
      <p className="search-empty-hint">
        {indexStatus && indexStatus.indexed_docs > 0
          ? `${indexStatus.indexed_docs.toLocaleString()} files indexed for inside-file search.`
          : "File contents are indexed from your chosen folders."}
      </p>
    </div>
  );
}
