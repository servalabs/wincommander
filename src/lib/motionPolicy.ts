// src/lib/motionPolicy.ts
//
// Single source of truth for whether animations should be OFF, and for
// applying the resulting `wc-no-motion` html class. Centralizes what used to
// live inline in AppShell so the same rules apply everywhere — including
// BEFORE React renders (see main.tsx), so the splash screen honors them too.
//
// Precedence: explicit user choice (localStorage "wc-motion") > OS
// prefers-reduced-motion > Windows 10 / low-spec hardware default.

/** Logical CPU cores. Assume a healthy 8 when the API is unavailable so we
 *  never disable motion on a capable machine that just hides the value. */
function coreCount(): number {
  return typeof navigator !== "undefined" && navigator.hardwareConcurrency
    ? navigator.hardwareConcurrency
    : 8;
}

/** Approx device RAM in GB. Chromium's `navigator.deviceMemory` is rounded and
 *  capped at 8 — fine for a "< 8 GB" threshold (a 6 GB box reports 4). Falls
 *  back to 8 when unavailable so we don't disable motion on unknown hardware. */
function deviceMemoryGb(): number {
  const mem =
    typeof navigator !== "undefined"
      ? (navigator as Navigator & { deviceMemory?: number }).deviceMemory
      : undefined;
  return typeof mem === "number" && mem > 0 ? mem : 8;
}

/** Underpowered machines (4 or fewer cores OR < 8 GB RAM) default to motion OFF: the
 *  animated dashboard radar and canvas effects are the dominant CPU cost, and
 *  weak integrated GPUs rasterize them on the CPU. Users can still opt back in
 *  via the "Disable all animations" toggle (which persists an explicit choice
 *  that overrides this default). */
export function isLowSpecHardwareProfile(cores: number, ramGb: number): boolean {
  return cores <= 4 || ramGb < 8;
}

export function isLowSpecHardware(): boolean {
  return isLowSpecHardwareProfile(coreCount(), deviceMemoryGb());
}

/** The startup probe supplies a friendly OS name; use it instead of the browser
 * user-agent because Windows 10 and 11 both commonly report `Windows NT 10.0`. */
export function isWindows10(osName?: string | null): boolean {
  return typeof osName === "string" && /windows\s+10\b/i.test(osName);
}

/** Additional facts available after the app's system probe completes. */
export function shouldReduceMotionForSystem(input: {
  osName?: string | null;
  ramTotalGb?: number | null;
}): boolean {
  return isWindows10(input.osName) ||
    (typeof input.ramTotalGb === "number" && input.ramTotalGb < 8);
}

function osPrefersReduced(): boolean {
  return (
    typeof window !== "undefined" &&
    !!window.matchMedia &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/** Resolve whether animations should be disabled right now. An explicit user
 *  choice always wins; otherwise fall back to the OS preference or the
 *  low-spec hardware default. */
export function resolveMotionDisabled(): boolean {
  const pref =
    typeof localStorage !== "undefined" ? localStorage.getItem("wc-motion") : null;
  if (pref === "0") return true; // user explicitly disabled
  if (pref === "1") return false; // user explicitly enabled
  return osPrefersReduced() || isLowSpecHardware();
}

/** Apply (or re-apply) the motion classes to <html>. Idempotent; safe to call
 *  repeatedly. Keeps `--motion` at a non-zero scalar because several CSS rules
 *  divide by it — zero would make animated UI vanish instead of going static. */
export function applyMotionClass(): void {
  if (typeof document === "undefined") return;
  const pref =
    typeof localStorage !== "undefined" ? localStorage.getItem("wc-motion") : null;
  const disabled = resolveMotionDisabled();
  document.documentElement.style.setProperty("--motion", "1");
  document.documentElement.classList.toggle("wc-no-motion", disabled);
  document.documentElement.classList.toggle("wc-motion-enabled", pref === "1");
}
