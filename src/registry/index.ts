// src/registry/index.ts
//
// ═══════════════════════════════════════════════════════════════════════
// TOGGLE REGISTRY — Single entry point for all toggle definitions
// ═══════════════════════════════════════════════════════════════════════
//
// USAGE:
//   import { ALL_TOGGLES, ALL_SECTIONS, getRadarToggles } from '../registry';
//
// WHY NOT NETWORK?
//   The network panel uses dynamic blocklists (fetched from backend) and
//   a complex DNS provider dropdown. These aren't simple on/off toggles,
//   so they don't fit the ToggleDef pattern. Network keeps its own UI.

import type { ToggleDef, SectionDef } from "../types/toggles";
import { PRIVACY_TOGGLES, PRIVACY_SECTIONS } from "./privacy.toggles";
import { TWEAKS_TOGGLES, TWEAKS_SECTIONS } from "./tweaks.toggles";
import { CAPABILITY_TOGGLES, CAPABILITY_SECTIONS } from "./capabilities.toggles";
import { ARGUS_TOGGLES } from "./argus.toggles";

// ── Combined arrays ──────────────────────────────────────────────────

export const ALL_TOGGLES: ToggleDef[] = [
  ...PRIVACY_TOGGLES,
  ...TWEAKS_TOGGLES,
  ...CAPABILITY_TOGGLES,
  ...ARGUS_TOGGLES,
];

export const ALL_SECTIONS: SectionDef[] = [
  ...PRIVACY_SECTIONS,
  ...TWEAKS_SECTIONS,
  ...CAPABILITY_SECTIONS,
];

// ── Lookup helpers ───────────────────────────────────────────────────

/** Find a toggle by its unique id */
export function getToggleById(id: string): ToggleDef | undefined {
  return ALL_TOGGLES.find((t) => t.id === id);
}

// ── Radar helpers ────────────────────────────────────────────────────

/** All toggles that appear in the dashboard radar scan.
 *  Replaces the hardcoded SCAN_CHECKS arrays in useDashboardRadar
 *  and the Help & Setup scan. */
export function getRadarToggles(): ToggleDef[] {
  return ALL_TOGGLES.filter((t) => t.radar);
}

/** Safe, score-affecting settings that should still surface in the dashboard
 * radar even when their registry row doesn't opt into the full scan. */
export function getRadarRecommendationToggles(): ToggleDef[] {
  return ALL_TOGGLES.filter((t) => {
    if (t.radar) return false;
    if (t.noNeedsAttention) return false;
    if (t.isAction) return false;
    if (!t.privacyScore && !t.cleanupScore) return false;
    if (!t.safeDefault && !t.defaultOn) return false;
    if (t.irreversible || t.reducesSecurity || t.defenderFlagged) return false;
    return true;
  });
}

/** Toggles whose user intent can be re-applied from the radar drift list.
 * Irreversible commands are excluded; action-style rows are allowed when they
 * still expose ideal/current state because the owner wants one-shot drift repair. */
export function getRadarDriftToggles(): ToggleDef[] {
  // `radar-only` entries describe bespoke cards whose actions use direct
  // Tauri commands. They are findings metadata, not generic backend toggles,
  // and must never be sent through executeBackendCommand by auto-heal.
  return ALL_TOGGLES.filter((t) => !t.irreversible && t.section !== "radar-only");
}

// ── Setup helpers ────────────────────────────────────────────────────

/** Toggles that are forcefully applied during setup,
 *  regardless of scan results. */
export function getAlwaysApplyToggles(): ToggleDef[] {
  return ALL_TOGGLES.filter((t) => t.alwaysApplyOnFirstRun);
}

// ── Score helpers ────────────────────────────────────────────────────

/** All toggles that contribute to Privacy Score (privacy + tweaks). */
export function getPrivacyScoreToggles(): ToggleDef[] {
  return ALL_TOGGLES.filter((t) => t.privacyScore && t.privacyScore > 0);
}

// Re-export domain-level arrays for selective imports
export { PRIVACY_TOGGLES, PRIVACY_SECTIONS } from "./privacy.toggles";
export { TWEAKS_TOGGLES, TWEAKS_SECTIONS } from "./tweaks.toggles";
export { CAPABILITY_TOGGLES, CAPABILITY_SECTIONS } from "./capabilities.toggles";
export { ARGUS_TOGGLES } from "./argus.toggles";
