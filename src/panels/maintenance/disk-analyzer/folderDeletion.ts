export interface FolderDeleteReceipt {
  status: "delete_requested" | "partial" | "failed";
  processed: number;
  cacheInvalidated: boolean;
  message?: string | null;
}

interface FolderDeleteBackend {
  confirmDiskFolderDelete(scanId: string, itemId: number): Promise<string | null>;
  deleteConfirmedDiskFolder(scanId: string, itemId: number, token: string): Promise<FolderDeleteReceipt>;
}

export type FolderDeleteAttempt =
  | { kind: "cancelled"; attempted: false }
  | { kind: "receipt"; attempted: true; receipt: FolderDeleteReceipt }
  | { kind: "error"; attempted: boolean; error: unknown };

/** Do not send a deletion request unless the native confirmation granted a token. */
export async function performFolderDeletion(backend: FolderDeleteBackend, scanId: string, itemId: number): Promise<FolderDeleteAttempt> {
  let attempted = false;
  try {
    const token = await backend.confirmDiskFolderDelete(scanId, itemId);
    if (!token) return { kind: "cancelled", attempted: false };
    attempted = true;
    return { kind: "receipt", attempted: true, receipt: await backend.deleteConfirmedDiskFolder(scanId, itemId, token) };
  } catch (error) {
    return { kind: "error", attempted, error };
  }
}
