import { newDiagnosticOperationId, recordDiagnostic, type SafeDiagnosticInput } from "../lib/diagnostics";

type Lifecycle = "requested" | "applying" | "applied" | "verified";
type Outcome = "started" | "progress" | "succeeded" | "failed" | "degraded" | "cancelled" | "timed_out";
type Severity = "info" | "warn" | "error";
type SafeRdpContext = SafeDiagnosticInput["context"];

/**
 * Records only an opaque RDP state transition. Session identifiers, account
 * names, host names and backend output deliberately never cross this boundary.
 */
export function beginRdpOperation(action: string): string {
  void action;
  return newDiagnosticOperationId("rdp");
}

export function recordRdpDiagnostic(
  operationId: string,
  action: string,
  stage: string,
  lifecycle: Lifecycle,
  outcome: Outcome,
  severity: Severity,
  errorCode?: string,
  context?: SafeRdpContext,
): void {
  recordDiagnostic({
    operationId,
    feature: "rdp",
    action,
    stage,
    lifecycle,
    outcome,
    errorCode,
    severity,
    retryability: outcome === "failed" || outcome === "timed_out" ? "automatic" : "never",
    suggestedNextAction: outcome === "failed" || outcome === "timed_out" ? "retry" : "none",
    privacyClass: "local_sensitive",
    context,
  });
}
