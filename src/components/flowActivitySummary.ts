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

export interface FlowExecutionReceipt {
  completed?: unknown;
  failedActionCount?: unknown;
  actionCount?: unknown;
  dryRun?: unknown;
}

export function flowExecutionOutcome(payload: FlowExecutionReceipt): "simulated" | "completed" | "incomplete" | "unknown" {
  if (payload.dryRun === true) return "simulated";
  if (typeof payload.failedActionCount === "number" && Number.isSafeInteger(payload.failedActionCount)
    && payload.failedActionCount > 0) return "incomplete";
  if (payload.completed === true) return "completed";
  if (payload.completed === false) return "incomplete";
  return "unknown";
}

export function flowExecutionActivity(payload: FlowExecutionReceipt): string {
  const outcome = flowExecutionOutcome(payload);
  if (outcome === "simulated") return "Simulation ended; notifications and delays may have run";
  if (outcome === "completed") return "Actions reported completed";
  if (outcome === "unknown") return "Flow ran; action outcome unknown";
  const count = payload.failedActionCount;
  return typeof count === "number" && Number.isSafeInteger(count) && count > 0
    ? `${count} action(s) failed or returned an unconfirmed result`
    : "Flow reported an incomplete or unconfirmed result";
}

export function flowExecutionSummary(payload: {
  flowId?: unknown;
  ruleId?: unknown;
  completed?: unknown;
  failedActionCount?: unknown;
  dryRun?: unknown;
  totalDurationMs?: unknown;
} & Record<string, unknown>): string {
  const reference = safeReference(payload.flowId) ?? safeReference(payload.ruleId);
  const outcome = flowExecutionOutcome(payload);
  const duration = typeof payload.totalDurationMs === "number"
    && Number.isFinite(payload.totalDurationMs)
    && payload.totalDurationMs >= 0
    && payload.totalDurationMs <= 86_400_000
    ? ` after ${Math.round(payload.totalDurationMs)}ms`
    : "";
  const description = outcome === "simulated" ? "simulation ended" : outcome === "unknown" ? "outcome unknown" : outcome === "incomplete" ? "incomplete or unconfirmed" : "completed";
  return `FLOW execution ${description}${reference ? ` (flow ${reference})` : ""}${duration}`;
}

export function flowNotifySummary(payload: { severity?: unknown } & Record<string, unknown>): string {
  return `FLOW notification emitted (severity ${safeSeverity(payload.severity)})`;
}
