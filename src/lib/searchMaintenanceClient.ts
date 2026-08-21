import { clearCommand } from "./commandIds";

/** The subset of the backend executor used by Search maintenance. Keeping it
 * structural means this module has no React dependency and cannot bypass the
 * hook's loading, error, activity, or command allow-list handling. */
export interface SearchMaintenanceResult<T> {
  success: boolean;
  data?: T;
  error?: string;
}

export type SearchMaintenanceExecutor = <T>(
  command: string,
  args?: Record<string, string | number | boolean>,
) => Promise<SearchMaintenanceResult<T>>;

export interface SearchMaintenanceFile {
  name: string;
  sizeKB: number;
  modified: string;
}

export interface SearchIndexInfo {
  files: Array<SearchMaintenanceFile & { label: string }>;
  total: number;
  totalSizeMB: number;
  wsearchState: string;
}

export interface SearchMaintenanceInfo {
  files: SearchMaintenanceFile[];
  total: number;
  totalSizeMB: number;
}

/** Stable, typed command client for Windows Search artefacts. The caller
 * supplies the established executor so telemetry and error handling remain
 * centralised in `useBackend`. */
export function createSearchMaintenanceClient(execute: SearchMaintenanceExecutor) {
  return {
    getSearchIndexInfo: () => execute<SearchIndexInfo>("Get-SearchIndexInfo"),
    getExplorerSearchHistoryInfo: () =>
      execute<SearchMaintenanceInfo>("Get-ExplorerSearchHistoryInfo"),
    getSearchPersonalizationInfo: () =>
      execute<SearchMaintenanceInfo>("Get-SearchPersonalizationInfo"),
    clearSearchIndex: () =>
      execute<{ status: string; removedItems: number }>(clearCommand("SearchIndex")),
    clearExplorerSearchHistory: () =>
      execute<{ ok: boolean; stdout: string }>(clearCommand("ExplorerSearchHistory")),
    clearSearchPersonalizationData: () =>
      execute<{ ok: boolean; stdout: string }>(clearCommand("SearchPersonalizationData")),
  };
}
