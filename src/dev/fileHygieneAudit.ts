import type { DuplicateScan } from "../hooks/useBackend";

const SCAN: DuplicateScan = {
  groups: [
    { id: "audit-archive", size: 64 * 1024 ** 2, reclaimableBytes: 64 * 1024 ** 2, files: [
      { id: "archive-original", name: "project.zip", path: "C:\\Audit\\Projects\\project.zip", size: 64 * 1024 ** 2 },
      { id: "archive-copy", name: "project-backup.zip", path: "C:\\Audit\\Downloads\\project-backup.zip", size: 64 * 1024 ** 2 },
    ] },
    { id: "audit-document", size: 2 * 1024 ** 2, reclaimableBytes: 2 * 1024 ** 2, files: [
      { id: "document-original", name: "notes.pdf", path: "C:\\Audit\\Documents\\notes.pdf", size: 2 * 1024 ** 2 },
      { id: "document-copy", name: "notes-copy.pdf", path: "C:\\Audit\\Downloads\\notes-copy.pdf", size: 2 * 1024 ** 2 },
    ] },
  ], scannedFiles: 142, skippedEntries: 0, truncated: false, cancelled: false,
};

export function fileHygieneAuditResponse(command: string, args: Record<string, unknown>): unknown {
  if (command === "duplicate_finder_scan") return structuredClone(SCAN);
  if (command === "plugin:dialog|open") {
    const options = args.options as { title?: string; directory?: boolean } | undefined;
    if (options?.directory && options.title === "Choose folders to inspect") return ["C:\\Audit"];
  }
  return undefined;
}
