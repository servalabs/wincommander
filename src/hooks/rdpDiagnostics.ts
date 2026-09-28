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
  // The RDP watchers poll frequently. Keep only a change of safe state for an
  // action, rather than filling the seven-day timeline with one entry per
  // poll. The map is bounded by the caller's fixed action names.
  const lastStateByAction = new Map<string, string>();
  return (...args) => {
    if (!enabled) return;
    const [, action, stage, lifecycle, outcome, severity, errorCode] = args;
    const state = `${stage}:${lifecycle}:${outcome}:${severity}:${errorCode ?? ""}`;
    if (lastStateByAction.get(action) === state) return;
    lastStateByAction.set(action, state);
    write(...args);
  };
}

export type RdpConsoleMirrorInput = [
  action: string,
  stage: string,
  lifecycle: Lifecycle,
  outcome: Outcome,
  severity: Severity,
  errorCode?: string,
];

/**
 * Mirrors a console branch as a structured, identity-free timeline state.
 * Callers supply fixed labels only: never raw console text, backend payloads,
 * session ids, account names, addresses, or idle values.
 */
export function createRdpConsoleMirror(
  enabled: boolean,
  write: (...args: RdpDiagnosticArguments) => void = recordRdpDiagnostic,
): (input: RdpConsoleMirrorInput) => void {
  const record = createRdpDiagnosticRecorder(enabled, write);
  return ([action, stage, lifecycle, outcome, severity, errorCode]) => {
    record(beginRdpOperation(action), action, stage, lifecycle, outcome, severity, errorCode);
  };
}
