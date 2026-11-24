// src/panels/secret/stealthMode.ts
//
// One Stealth mode switch drives four existing concealment features. Order,
// status and input checks are pure and live here; the panel runs the real steps.

import { validatePinInput } from "../privacy/pinRules";

export type StealthPartId = "cover" | "backend" | "hide" | "lock";
export type StealthState = Record<StealthPartId, boolean>;
export type StealthMode = "off" | "partial" | "on";

// Blank names fall back to this: the backend would otherwise show the real app name, which is no cover.
export const DEFAULT_COVER_NAME = "Calculator";

export function resolveCoverName(name: string): string {
  return name.trim() || DEFAULT_COVER_NAME;
}

export const STEALTH_LABELS: Record<StealthPartId, string> = {
  lock: "Calculator lock",
  cover: "Cover identity",
  hide: "Hide WinCommander",
  backend: "Hide backend apps",
};

// The lock goes last on the way in: it switches the app to the calculator, so no step can follow it.
const ON_ORDER: readonly StealthPartId[] = ["cover", "backend", "hide", "lock"];
const OFF_ORDER: readonly StealthPartId[] = ["hide", "backend", "cover", "lock"];

// Cover identity is a paid feature; unpaid installs run the other three.
export function stealthParts(hasPaid: boolean): StealthPartId[] {
  return ON_ORDER.filter((id) => hasPaid || id !== "cover");
}

export function stealthSummary(state: StealthState, hasPaid: boolean): { mode: StealthMode; active: number; total: number } {
  const parts = stealthParts(hasPaid);
  const active = parts.filter((id) => state[id]).length;
  const anyActive = ON_ORDER.some((id) => state[id]);
  const mode: StealthMode = !anyActive ? "off" : active === parts.length ? "on" : "partial";
  return { mode, active, total: parts.length };
}

export type StealthTone = StealthMode | "busy";

export function stealthStatus(summary: { mode: StealthMode; active: number; total: number }, busy: boolean): { label: string; tone: StealthTone } {
  if (busy) return { label: "Working…", tone: "busy" };
  if (summary.mode === "on") return { label: "On", tone: "on" };
  if (summary.mode === "partial") return { label: `Partly on (${summary.active} of ${summary.total})`, tone: "partial" };
  return { label: "Off", tone: "off" };
}

// Turning off also undoes a lapsed-plan cover; turning on runs only what applies and is not done yet.
export function stealthPending(state: StealthState, hasPaid: boolean, direction: "on" | "off"): StealthPartId[] {
  if (direction === "off") return OFF_ORDER.filter((id) => state[id]);
  const parts = stealthParts(hasPaid);
  return ON_ORDER.filter((id) => parts.includes(id) && !state[id]);
}

export function stealthSetupError(input: { hasRealPin: boolean; pin: string }): string | null {
  if (!input.hasRealPin) {
    const pinError = validatePinInput(input.pin.trim());
    if (pinError) return `Set the calculator PIN first: ${pinError}.`;
  }
  return null;
}

export async function runStealthSteps(
  ids: readonly StealthPartId[],
  run: (id: StealthPartId) => Promise<boolean>,
): Promise<{ done: StealthPartId[]; failed: StealthPartId | null }> {
  const done: StealthPartId[] = [];
  for (const id of ids) {
    let ok = false;
    try { ok = await run(id); } catch { ok = false; }
    if (!ok) return { done, failed: id };
    done.push(id);
  }
  return { done, failed: null };
}
