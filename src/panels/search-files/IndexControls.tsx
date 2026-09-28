import { Icon } from "@/components/ui/icon";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { IndexedFoldersManager, type IndexedFoldersProps } from "./IndexedFolders";
import type { IndexStatus } from "@/types/wincmd-search";

interface IndexControlsProps extends IndexedFoldersProps {
  expanded: boolean;
  onToggle: () => void;
  indexStatus: IndexStatus | null;
  indexDisplayError: string | null;
  foldersReindexing: boolean;
  managementError: string | null;
}

export default function IndexControls({ expanded, onToggle, indexStatus, indexDisplayError, foldersReindexing, managementError, ...folders }: IndexControlsProps) {
  const showFolders = expanded || folders.roots.length === 0;
  return (
    <section className="sfp-index-controls" aria-label="Content search index">
      <div className="sfp-index-toolbar">
        <span className="sfp-index-status" role="status" aria-live="polite">
          {indexStatus?.is_indexing ? (
            <><Spinner size={12} /> Indexing… {indexStatus.indexed_docs.toLocaleString()} of {(indexStatus.indexed_docs + indexStatus.pending_docs).toLocaleString()} files</>
          ) : indexStatus && indexStatus.indexed_docs > 0 ? (
            `${indexStatus.indexed_docs.toLocaleString()} files indexed`
          ) : "Choose folders to search inside files"}
          {foldersReindexing && " · updating folders…"}
          {indexDisplayError && <span className="sfp-index-status__error"> · {indexDisplayError}</span>}
        </span>
        <Button
          size="sm"
          variant="ghost"
          aria-expanded={showFolders}
          aria-controls="sfp-indexed-folders"
          title="Choose which folders are indexed for inside-file search"
          onClick={onToggle}
        >
          <Icon icon="cog" size={14} />
          Indexed folders
        </Button>
      </div>
      {managementError && <p className="sfp-index-status__error" role="alert">{managementError}</p>}
      {showFolders && (
        <div id="sfp-indexed-folders" className="sfp-index-folders">
          <IndexedFoldersManager {...folders} foldersReindexing={foldersReindexing} />
        </div>
      )}
    </section>
  );
}
