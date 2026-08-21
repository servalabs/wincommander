// src/lib/evidence.ts
//
// Frontend bridge to the free-tier evidence.timeline ledger (Rust:
// commander-free/src/evidence.rs). Records what WinCommander detected/did
// to a local append-only JSONL feed and reads it back for the timeline UI.
//
// Free tier = plain, unsigned, local. The paid evidence.vault upgrade adds
// the court-admissible signed layer Pro-side; the timeline UI consumes
// whichever is present.

import { invoke } from "@tauri-apps/api/core";

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
    /* best-effort */
  });
}

/** Read the most recent entries, newest first. Returns [] on any error. */
export async function readEvidence(limit = 200): Promise<EvidenceEntry[]> {
  try {
    return await invoke<EvidenceEntry[]>("evidence_read", { limit });
  } catch {
    return [];
  }
}

/** Clear the local ledger (user-initiated). */
export async function clearEvidence(): Promise<void> {
  try {
    await invoke("evidence_clear");
  } catch {
    /* ignore */
  }
}
