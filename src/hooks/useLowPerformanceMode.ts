// src/hooks/useLowPerformanceMode.ts
//
// Resolves Low Performance Mode from the persisted setting plus the startup
// system probe, and keeps the `wc-no-motion` html class in sync so CSS and
// framer-motion both follow the same decision.
//
// Mirrors useMotionPreference's shape deliberately: one resolver, consumed in
// App.tsx, so animations and polling can never end up in disagreement.

import { useEffect } from "react";
import { useAppState } from "../context/AppContext";
import { applyMotionClass } from "../lib/motionPolicy";
import {
  parseLowPerformancePref,
  resolveLowPerformance,
  type LowPerformancePref,
} from "../lib/lowPerformance";

export interface LowPerformanceState {
  /** True when the mode is active, whether chosen or auto-detected. */
  active: boolean;
  /** The persisted preference behind that decision. */
  pref: LowPerformancePref;
  /** Probe values used for the "auto" decision, for the settings UI to explain itself. */
  cores: number | null;
  ramGb: number | null;
}

export default function useLowPerformanceMode(): LowPerformanceState {
  const { appSettings, systemInfo } = useAppState();

  const pref = parseLowPerformancePref(appSettings?.app?.lowPerformanceMode);

  // The startup probe is authoritative for RAM. Core count is not part of the
  // probe, so fall back to the browser's logical-core count — which Chromium
  // reports accurately on Windows, unlike deviceMemory which is rounded and
  // capped at 8.
  const ramGb = typeof systemInfo?.ramTotalGb === "number" ? systemInfo.ramTotalGb : null;
  const cores =
    typeof navigator !== "undefined" && navigator.hardwareConcurrency
      ? navigator.hardwareConcurrency
      : null;

  const active = resolveLowPerformance({ pref, cores, ramGb });

  // Keep CSS in step. applyMotionClass() already owns the baseline decision
  // (explicit wc-motion choice > OS preference > low-spec hardware); this only
  // forces the class ON when the mode is active, and otherwise hands control
  // back rather than forcing it off — so a user who independently disabled
  // animations keeps them disabled.
  useEffect(() => {
    if (typeof document === "undefined") return;
    if (active) {
      document.documentElement.classList.add("wc-no-motion");
    } else {
      applyMotionClass();
    }
  }, [active]);

  return { active, pref, cores, ramGb };
}
