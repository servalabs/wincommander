/**
 * Debug-only, in-memory diagnostics feed. This deliberately is not a console
 * interceptor: intercepting console output would retain arbitrary paths,
 * payloads, and exception details. Callers may submit only already-safe flow
 * summaries, and the store strips the one dynamic reference those summaries
 * contain before keeping a bounded local snapshot.
 */
export const MAX_DEVELOPER_DIAGNOSTIC_ENTRIES = 80;

export type DeveloperDiagnosticEntry = Readonly<{
  id: number;
  occurredAt: string;
  summary: string;
}>;

let enabled = false;
let nextId = 1;
let entries: readonly DeveloperDiagnosticEntry[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  listeners.forEach((listener) => listener());
}

/** Native-debug and explicit opt-in are both required. Disabling clears RAM. */
export function configureDeveloperDiagnosticsLog(
  isNativeDebugBuild: boolean,
  userOptedIn: boolean | undefined,
): void {
  const nextEnabled = isNativeDebugBuild && userOptedIn === true;
  const hadEntries = entries.length > 0;
  const changed = enabled !== nextEnabled;
  enabled = nextEnabled;
  if (!enabled) entries = [];
  if (changed || (!enabled && hadEntries)) notify();
}

function safeFlowSummary(value: string): string | null {
  // FlowActivityLogger creates these fixed, payload-free summaries. Do not
  // widen this to arbitrary console text: that is exactly what this surface
  // must never retain or show.
  if (!/^(activity logger armed|DETECT |FLOW )/.test(value)) return null;
  const withoutReference = value.replace(/ \(flow [^)]+\)/g, "");
  if (withoutReference.length === 0 || withoutReference.length > 180) return null;
  // Defence in depth for future callers: reject path, URL, UNC, and email
  // shaped content rather than trying to redact arbitrary developer output.
  if (/(?:[A-Za-z]:[\\/]|\\\\|https?:\/\/|[\w.+-]+@[\w.-]+\.[A-Za-z]{2,})/.test(withoutReference)) return null;
  return withoutReference;
}

/** Records one allow-listed, privacy-safe FlowActivityLogger summary. */
export function recordDeveloperFlowDiagnostic(summary: string): void {
  if (!enabled) return;
  const safeSummary = safeFlowSummary(summary);
  if (!safeSummary) return;
  entries = [
    ...entries,
    Object.freeze({ id: nextId++, occurredAt: new Date().toISOString(), summary: safeSummary }),
  ].slice(-MAX_DEVELOPER_DIAGNOSTIC_ENTRIES);
  notify();
}

export function getDeveloperDiagnosticsSnapshot(): readonly DeveloperDiagnosticEntry[] {
  return entries;
}

export function clearDeveloperDiagnosticsLog(): void {
  if (entries.length === 0) return;
  entries = [];
  notify();
}

export function subscribeDeveloperDiagnostics(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
