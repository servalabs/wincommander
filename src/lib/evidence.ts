// src/lib/evidence.ts
//
// Frontend bridge to the free-tier evidence.timeline ledger (Rust:
// commander-free/src/evidence.rs). Records what WinCommander detected/did
// to an encrypted local feed and reads it back for the timeline UI.
//
// Free tier = encrypted, unsigned, local. The paid evidence.vault upgrade adds
// the court-admissible signed layer Pro-side; the timeline UI consumes
// whichever is present.

import { invoke } from "@tauri-apps/api/core";
import { pushNotification } from "./notificationStore";

export type EvidenceSeverity = "info" | "warn" | "danger";

export interface EvidenceEntry {
  /** ISO-8601 UTC. */
  time: string;
  /** "monitor" | "network" | "lockdown" | "privacy" | "system" | "flow" | … */
  source: string;
  severity: EvidenceSeverity;
  summary: string;
  detail?: string;
}

/**
 * Append one entry to the local evidence ledger. Best-effort and fire-and-
 * forget — a ledger write must never disrupt the detection it's recording,
 * so failures are swallowed.
 */
export function recordEvidence(
  source: string,
  severity: EvidenceSeverity,
  summary: string,
  detail?: string,
): void {
  invoke("evidence_record", { source, severity, summary, detail: detail ?? null }).catch(() => {
    pushNotification("warn", "An activity could not be recorded. Check activity history before relying on it.", undefined, "alert", "evidence-record-failure");
  });
}

/** Read the most recent entries, newest first. */
export async function readEvidence(limit = 200): Promise<EvidenceEntry[]> {
  return invoke<EvidenceEntry[]>("evidence_read", { limit });
}

/** Clear the local ledger (user-initiated). */
export async function clearEvidence(): Promise<void> {
  await invoke("evidence_clear");
}

export async function getEvidenceStatus(): Promise<{ continuityUnverified: boolean }> {
  const value = await invoke<unknown>("evidence_status");
  if (!value || typeof value !== "object" || typeof (value as { continuityUnverified?: unknown }).continuityUnverified !== "boolean") {
    throw new Error("Evidence status is unavailable.");
  }
  return value as { continuityUnverified: boolean };
}
