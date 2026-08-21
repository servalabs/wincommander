// SPDX-License-Identifier: AGPL-3.0-or-later
// Shared, content-free status contract for panel, overlay, and Explorer entry.
export type FileSearchDiagnosticStage = "service_unavailable" | "index_unavailable" | "query_accepted" | "scope_applied" | "result_incomplete" | "timeout" | "cancelled";
export interface FileSearchDiagnostic { stage: FileSearchDiagnosticStage; message: string; }
const MESSAGES: Record<FileSearchDiagnosticStage, string> = {
  service_unavailable: "File search service is unavailable.",
  index_unavailable: "The local search index is not ready.",
  query_accepted: "Search query accepted.",
  scope_applied: "Selected search scope applied.",
  result_incomplete: "Results are incomplete; refine the search or wait for indexing.",
  timeout: "Search timed out before completion.",
  cancelled: "Search was cancelled.",
};
export function fileSearchDiagnostic(stage: FileSearchDiagnosticStage): FileSearchDiagnostic { return { stage, message: MESSAGES[stage] }; }
