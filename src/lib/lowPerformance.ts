// src/lib/lowPerformance.ts
//
// Low Performance Mode — one switch that turns off the two things that actually
// cost a weak or heavily-shared machine: animations, and the periodic panel
// polling that spawns a cold powershell.exe every 10 seconds.
//
// WHY IT REPLACES "Disable all animations": animations were only ever half the
// bill. Each active-panel refresh spawns a brand-new PowerShell host
// (backend.rs::build_powershell_command — no runspace reuse, no caching), and on a
// multi-user Windows Server every logged-in session pays that independently. A
// user on a constrained box who reaches for "disable animations" wants the machine
// to stop working hard; silencing framer-motion while still shelling out every 10s
// does not deliver that.
//
// Precedence, highest first:
//   1. explicit user choice ("on" / "off")
//   2. hardware auto-detection ("auto", the default)
//
// The hardware rule reuses isLowSpecHardwareProfile() from motionPolicy.ts rather
// than restating the thresholds, so motion and polling can never disagree about
// whether this machine counts as low-spec.

import { isLowSpecHardwareProfile } from "./motionPolicy";

/** Persisted preference. "auto" defers to hardware; the others are explicit. */
export type LowPerformancePref = "auto" | "on" | "off";

export const LOW_PERFORMANCE_DEFAULT: LowPerformancePref = "auto";

/** Narrows an untrusted settings value to a known preference. */
export function parseLowPerformancePref(value: unknown): LowPerformancePref {
  return value === "on" || value === "off" || value === "auto"
    ? value
    : LOW_PERFORMANCE_DEFAULT;
}

/**
 * Resolve whether Low Performance Mode is active right now.
 *
 * `cores` and `ramGb` come from the startup system probe when available. They are
 * optional because the probe completes asynchronously — until it does, callers
 * pass undefined and get the browser-derived fallback inside
 * isLowSpecHardwareProfile's callers. Passing explicit values keeps this function
 * pure and directly testable.
 */
export function resolveLowPerformance(input: {
  pref: LowPerformancePref;
  cores?: number | null;
  ramGb?: number | null;
}): boolean {
  if (input.pref === "on") return true;
  if (input.pref === "off") return false;

  // "auto" — decide from hardware. Unknown values must NOT opt a machine in:
  // a capable box that simply hides its specs should keep full behaviour, so
  // each unknown falls back to a comfortably-above-threshold value.
  const cores = typeof input.cores === "number" && input.cores > 0 ? input.cores : 8;
  const ramGb = typeof input.ramGb === "number" && input.ramGb > 0 ? input.ramGb : 16;
  return isLowSpecHardwareProfile(cores, ramGb);
}

/** Human-readable reason, for the settings UI to explain an automatic decision. */
export function describeLowPerformanceReason(input: {
  pref: LowPerformancePref;
  cores?: number | null;
  ramGb?: number | null;
  active: boolean;
}): string {
  if (input.pref === "on") return "Turned on manually.";
  if (input.pref === "off") return "Turned off manually.";
  if (!input.active) return "This PC has enough memory and cores, so it stays off.";

  const reasons: string[] = [];
  if (typeof input.cores === "number" && input.cores > 0 && input.cores <= 4) {
    reasons.push(`${input.cores} CPU core${input.cores === 1 ? "" : "s"}`);
  }
  if (typeof input.ramGb === "number" && input.ramGb > 0 && input.ramGb < 8) {
    reasons.push(`${input.ramGb} GB memory`);
  }
  return reasons.length
    ? `Turned on automatically — this PC has ${reasons.join(" and ")}.`
    : "Turned on automatically for this PC's hardware.";
}
