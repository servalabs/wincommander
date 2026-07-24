// src/panels/search-files/IndexedFolders.tsx
//
// Indexed-folders management box (list + Add folder + Rescan + Re-index) shown
// behind the "Indexed folders" gear, plus the first-run onboarding card
// when nothing is indexed yet. Pure renderer.

import { Icon } from "@/components/ui/icon";
import { Button } from "@/components/ui/button";

interface IndexedFoldersProps {
  roots: string[];
  reindexing: boolean;
  rescanning: boolean;
  onReindex: () => void;
  onRescan: () => void;
  onAddFolders: () => void;
  onRemoveFolder: (root: string) => void;
}

export function IndexedFoldersManager({ roots, reindexing, rescanning, onReindex, onRescan, onAddFolders, onRemoveFolder }: IndexedFoldersProps) {
  const busy = reindexing || rescanning;
  return (
    <div className="sfp-folders-section">
      <div className="sfp-folders-header">
        <span className="sfp-folders-label">Indexed folders</span>
        <div className="sfp-folders-header-actions">
          <Button
            size="sm"
            variant="ghost"
            onClick={onRescan}
            disabled={busy}
            title="Scan for new, changed, or previously-missed files — keeps existing results searchable"
          >
            <Icon icon="refresh" size={14} />
            {rescanning ? "Rescanning…" : "Rescan"}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={onReindex}
            disabled={busy}
            title="Rebuild the whole index from scratch (clears removed files; search is unavailable until it finishes)"
          >
            <Icon icon="reset" size={14} />
            {reindexing ? "Re-indexing…" : "Re-index"}
          </Button>
          <Button size="sm" variant="ghost" onClick={onAddFolders} title="Add folder to index">
            <Icon icon="folder-new" size={14} />
            Add folder
          </Button>
        </div>
      </div>
      {roots.length === 0 ? (
        <p className="sfp-folders-empty">No folders indexed — click "Add folder" to start.</p>
      ) : (
        <ul className="sfp-folders-list">
          {roots.map((root) => (
            <li key={root} className="sfp-folders-row">
              <Icon icon="folder-close" size={14} className="sfp-folders-icon" />
              <span className="sfp-folders-path" title={root}>{root}</span>
              <Button
                size="icon"
                variant="ghost"
                title="Remove folder"
                aria-label="Remove folder"
                onClick={() => onRemoveFolder(root)}
              >
                <Icon icon="cross" size={14} />
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function NoFoldersOnboarding({ onAddFolders }: { onAddFolders: () => void }) {
  return (
    <div className="sfp-no-folders-state">
      <div className="sfp-no-folders-icon" aria-hidden="true">
        <Icon icon="folder-new" size={40} />
      </div>
      <p className="sfp-no-folders-heading">No folders indexed yet</p>
      <p className="sfp-no-folders-hint">
        Add at least one folder so WinCommander can index its contents for full-text search.
      </p>
      <Button variant="primary" onClick={onAddFolders} className="sfp-no-folders-cta">
        <Icon icon="folder-new" size={14} />
        Add folder
      </Button>
    </div>
  );
}
