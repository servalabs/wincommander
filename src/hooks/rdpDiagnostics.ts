import { newDiagnosticOperationId, recordDiagnostic, type SafeDiagnosticInput } from "../lib/diagnostics";

type Lifecycle = "requested" | "applying" | "applied" | "verified";
type Outcome = "started" | "progress" | "succeeded" | "failed" | "degraded" | "cancelled" | "timed_out";
type Severity = "info" | "warn" | "error";
type SafeRdpContext = SafeDiagnosticInput["context"];
export type RdpDiagnosticArguments = [
  operationId: string,
  action: string,
  stage: string,
  lifecycle: Lifecycle,
  outcome: Outcome,
  severity: Severity,
  errorCode?: string,
  context?: SafeRdpContext,
];

/**
 * Records only an opaque RDP state transition. Session identifiers, account
 * names, host names and backend output deliberately never cross this boundary.
 */
export function beginRdpOperation(action: string): string {
  void action;
  return newDiagnosticOperationId("rdp");
}

export function recordRdpDiagnostic(
  ...[operationId, action, stage, lifecycle, outcome, severity, errorCode, context]: RdpDiagnosticArguments
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

/**
 * The persisted RDP diagnostic-history preference is intentionally the one
 * gate for every RDP timeline producer. A disabled preference must not create
 * a new record, including coarse monitor transitions.
 */
export function createRdpDiagnosticRecorder(
  enabled: boolean,
  write: (...args: RdpDiagnosticArguments) => void = recordRdpDiagnostic,
): (...args: RdpDiagnosticArguments) => void {
  return (...args) => {
    if (enabled) write(...args);
  };
}
