// Global bridge from persisted native/service diagnostics to the in-app bell.
// The source event is already durable and redacted. This component stores only
// opaque event IDs as an idempotency cursor; it is not a second diagnostic log.
import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { diagnosticBellProjection } from "../lib/diagnosticNotification";
import { pushNotification } from "../lib/notificationStore";

type BridgeEvent = {
  eventId: string;
  operationId: string;
  occurredAt: string;
  feature: string;
  action: string;
  outcome: "started" | "progress" | "succeeded" | "failed" | "degraded" | "recovered" | "cancelled" | "timed_out";
  privacyClass?: "public" | "local_sensitive" | "restricted";
};

const CURSOR_KEY = "wc-diagnostic-bell-cursor-v1";
const RETENTION_MS = 7 * 24 * 60 * 60 * 1000;
const POLL_MS = 30_000;

function readCursor(now: number): Record<string, number> {
  try {
    const parsed = JSON.parse(localStorage.getItem(CURSOR_KEY) ?? "{}") as Record<string, number>;
    return Object.fromEntries(Object.entries(parsed).filter(([key, value]) => key.length <= 160 && Number.isFinite(value) && value >= now - RETENTION_MS));
  } catch {
    return {};
  }
}

function writeCursor(cursor: Record<string, number>): void {
  try { localStorage.setItem(CURSOR_KEY, JSON.stringify(cursor)); } catch { /* non-fatal idempotency cache */ }
}

function eventKey(event: BridgeEvent): string {
  return `${event.eventId}:${event.outcome}`.slice(0, 160);
}

function isBridgeEvent(value: unknown): value is BridgeEvent {
  if (!value || typeof value !== "object") return false;
  const event = value as Partial<BridgeEvent>;
  return typeof event.eventId === "string" && typeof event.operationId === "string"
    && typeof event.feature === "string" && typeof event.action === "string"
    && typeof event.outcome === "string";
}

export function projectPersistedDiagnostics(events: unknown[], now = Date.now()): void {
  const cursor = readCursor(now);
  let changed = false;
  for (const event of events) {
    if (!isBridgeEvent(event)) continue;
    const key = eventKey(event);
    if (cursor[key]) continue;
    const projection = diagnosticBellProjection({
      feature: event.feature,
      action: event.action,
      outcome: event.outcome,
      privacyClass: event.privacyClass ?? "local_sensitive",
    });
    if (!projection) continue;
    pushNotification(projection.severity, projection.message, undefined, projection.kind, event.operationId);
    cursor[key] = now;
    changed = true;
  }
  if (changed) writeCursor(cursor);
}

/** Mount once: native records are durably stored first, then safely mirrored to the bell. */
export default function DiagnosticEventBridge() {
  useEffect(() => {
    let active = true;
    const refresh = async () => {
      const [desktop, service] = await Promise.all([
        invoke<unknown[]>("get_diagnostic_events", { limit: 100 }).catch(() => []),
        invoke<unknown[]>("get_service_diagnostic_summaries", { limit: 100 }).catch(() => []),
      ]);
      if (active) projectPersistedDiagnostics([...desktop, ...service]);
    };
    void refresh();
    const timer = window.setInterval(() => { void refresh(); }, POLL_MS);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  return null;
}
