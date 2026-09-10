// The only frontend boundary for diagnostic Tauri commands. Keeping this
// boundary in hooks preserves the repository's IPC layering rule.
import { invoke } from "@tauri-apps/api/core";

export interface DiagnosticEventRequest {
  eventId: string;
  operationId: string;
  parentOperationId?: string;
  occurredAt: string;
  component: "desktop_ui";
  feature: string;
  action: string;
  stage: string;
  lifecycle: string;
  outcome: string;
  errorCode?: string;
  severity: string;
  retryability: string;
  suggestedNextAction: string;
  durationMs?: number;
  privacyClass: string;
  redactedContext: Record<string, string>;
}

export function getDiagnosticEvents(): Promise<unknown[]> {
  return invoke<unknown[]>("get_diagnostic_events", { limit: 100 });
}

export function getServiceDiagnosticSummaries(): Promise<unknown[]> {
  return invoke<unknown[]>("get_service_diagnostic_summaries", { limit: 100 });
}

export function getProDiagnosticSummaries(): Promise<unknown[]> {
  return invoke<unknown[]>("get_pro_diagnostic_summaries", { limit: 100 });
}

export function recordDiagnosticEvent(event: DiagnosticEventRequest): Promise<void> {
  return invoke("record_diagnostic_event", { event });
}
