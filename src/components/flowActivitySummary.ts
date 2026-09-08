// Safe, developer-only summaries for the FlowActivityLogger.
// Event payloads can contain user-authored rules, paths, SSIDs, or clipboard
// match labels. Do not pass them to DevTools or the console-log mirror.

function safeReference(value: unknown): string | undefined {
  return typeof value === "string" && /^[A-Za-z0-9._:-]{1,128}$/.test(value)
    ? value
    : undefined;
}

function safeSeverity(value: unknown): "info" | "warning" | "danger" | "unknown" {
  return value === "info" || value === "warning" || value === "danger"
    ? value
    : "unknown";
}

export function clipboardActivitySummary(payload: { severity?: unknown } & Record<string, unknown>): string {
  return `DETECT clipboard policy event (severity ${safeSeverity(payload.severity)})`;
}

export function flowDecisionSummary(payload: { ruleId?: unknown; reason?: unknown } & Record<string, unknown>): string {
  const decision = payload.reason === "admit"
    ? "admitted"
    : payload.reason === "refused"
      ? "refused"
      : "recorded";
  const reference = safeReference(payload.ruleId);
  return `FLOW decision ${decision}${reference ? ` (flow ${reference})` : ""}`;
}

export function flowExecutionSummary(payload: {
  flowId?: unknown;
  ruleId?: unknown;
  completed?: unknown;
  totalDurationMs?: unknown;
} & Record<string, unknown>): string {
  const reference = safeReference(payload.flowId) ?? safeReference(payload.ruleId);
  const outcome = payload.completed === true ? "completed" : "ended";
  const duration = typeof payload.totalDurationMs === "number"
    && Number.isFinite(payload.totalDurationMs)
    && payload.totalDurationMs >= 0
    && payload.totalDurationMs <= 86_400_000
    ? ` after ${Math.round(payload.totalDurationMs)}ms`
    : "";
  return `FLOW execution ${outcome}${reference ? ` (flow ${reference})` : ""}${duration}`;
}

export function flowNotifySummary(payload: { severity?: unknown } & Record<string, unknown>): string {
  return `FLOW notification emitted (severity ${safeSeverity(payload.severity)})`;
}
