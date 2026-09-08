// Safe frontend producer for the encrypted diagnostic event store.
// Features call this before a policy-derived bell projection. Never
// put user text, paths, camera data, clipboard content, or backend errors here.
import { invoke } from "@tauri-apps/api/core";
import { routeDiagnosticNotification } from "./diagnosticNotification";
import { pushDiagnosticNotification } from "./notificationStore";

export type DiagnosticLifecycle = "requested" | "delivered" | "acknowledged" | "applying" | "applied" | "verified";
export type DiagnosticOutcome = "started" | "progress" | "succeeded" | "failed" | "degraded" | "recovered" | "cancelled" | "timed_out";
export type DiagnosticSeverity = "debug" | "info" | "warn" | "error" | "critical";
export type DiagnosticRetryability = "never" | "manual" | "automatic";
export type DiagnosticPrivacyClass = "public" | "local_sensitive" | "restricted";

export interface SafeDiagnosticInput {
  operationId?: string;
  parentOperationId?: string;
  feature: string;
  action: string;
  stage: string;
  lifecycle: DiagnosticLifecycle;
  outcome: DiagnosticOutcome;
  errorCode?: string;
  severity: DiagnosticSeverity;
  retryability: DiagnosticRetryability;
  suggestedNextAction: string;
  durationMs?: number;
  privacyClass: DiagnosticPrivacyClass;
  /** Only fixed category/state/health metadata. The Rust writer rechecks it. */
  context?: Record<string, string | number | boolean | undefined>;
}

const SAFE_TOKEN = /^[A-Za-z0-9_-]{1,128}$/;
const SAFE_IDENTIFIER = /^[a-z0-9_-]{1,128}$/;
const SAFE_CODE = /^[A-Z0-9._-]{1,128}$/;
const CONTEXT_KEYS = new Set(["attempt", "build_version", "capability", "driver_state", "health", "os_error_code", "policy_version", "reason_category", "retry_count", "state"]);


function token(prefix: string): string {
  const suffix = globalThis.crypto?.randomUUID?.().replaceAll("-", "")
    ?? `${Date.now()}${Math.random().toString(36).slice(2)}`;
  return `${prefix}-${suffix}`.slice(0, 128);
}

export function newDiagnosticOperationId(feature: string): string {
  const prefix = feature.replace(/[^a-z0-9]/gi, "").toUpperCase().slice(0, 12) || "OP";
  return token(prefix);
}

function safeContext(context?: SafeDiagnosticInput["context"]): Record<string, string> {
  const output: Record<string, string> = {};
  for (const [key, value] of Object.entries(context ?? {})) {
    if (CONTEXT_KEYS.has(key) && value !== undefined) output[key] = String(value).slice(0, 128);
  }
  return output;
}

/** Persist a safe event. Failure is intentionally non-throwing so protection still works. */
export function recordDiagnostic(input: SafeDiagnosticInput): string {
  const operationId = input.operationId && SAFE_TOKEN.test(input.operationId)
    ? input.operationId : newDiagnosticOperationId(input.feature);
  const valid = SAFE_IDENTIFIER.test(input.feature) && SAFE_IDENTIFIER.test(input.action)
    && SAFE_IDENTIFIER.test(input.stage) && SAFE_IDENTIFIER.test(input.suggestedNextAction);
  if (!valid || (input.errorCode !== undefined && !SAFE_CODE.test(input.errorCode))) return operationId;
  const route = routeDiagnosticNotification(input);
  void invoke("record_diagnostic_event", {
    event: {
      eventId: token("evt"), operationId,
      parentOperationId: input.parentOperationId && SAFE_TOKEN.test(input.parentOperationId) ? input.parentOperationId : undefined,
      occurredAt: new Date().toISOString(), component: "desktop_ui",
      feature: input.feature, action: input.action, stage: input.stage,
      lifecycle: input.lifecycle, outcome: input.outcome, errorCode: input.errorCode,
      severity: input.severity, retryability: input.retryability,
      suggestedNextAction: input.suggestedNextAction,
      durationMs: Number.isFinite(input.durationMs) && input.durationMs! >= 0 ? Math.round(input.durationMs!) : undefined,
      privacyClass: input.privacyClass, redactedContext: safeContext(input.context),
    },
  }).then(() => {
    // The bell is a projection of a durable event, never a parallel error path.
    // It contains only a stable operation reference; details stay encrypted.
    if (route.bell) pushDiagnosticNotification(route.bell, operationId);
  }).catch(() => {});
  return operationId;
}
