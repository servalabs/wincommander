// src/hooks/useMotionPreference.ts
// Single resolver for the full/reduced motion preference.
//
// WHY ONE HOOK: four divergent reduced-motion paths existed before (index.css
// @media, v2-theme.css @media, SovereigntyRadar.css @media, html.wc-no-motion
// class). A single resolver lets <MotionConfig> silence ALL framer-motion
// surfaces in one stroke, while CSS defers to the wc-no-motion class.
//
// Returns 'reduced' when ANY of:
//   1. OS prefers-reduced-motion: reduce, unless the user explicitly enabled
//      app motion with localStorage "wc-motion"="1"
//   2. html.wc-no-motion class is present (Appearance drawer toggle or the
//      localStorage "wc-motion"="0" preference applied in AppShell)
//   3. App is in its low-profile posture (decoy mode) — motion suppressed
//      by behavior, not by name (DN-07 compliant).

import { useEffect, useState } from "react";
import { useAuthMode } from "../context/AuthModeContext";
import { useAppState } from "../context/AppContext";
import { applyMotionClass, shouldReduceMotionForSystem } from "../lib/motionPolicy";

export type MotionPreference = "full" | "reduced";

/** True when the OS has prefers-reduced-motion: reduce */
function osWantsReduced(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;

  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** True when the html element carries the wc-no-motion class */
function classWantsReduced(): boolean {
  if (typeof document === "undefined") return false;

  return document.documentElement.classList.contains("wc-no-motion");
}

function userExplicitlyEnabledMotion(): boolean {
  if (typeof window === "undefined") return false;

  return localStorage.getItem("wc-motion") === "1";
}

function resolvedWantsReduced(): boolean {
  return classWantsReduced() || (osWantsReduced() && !userExplicitlyEnabledMotion());
}

/**
 * Returns the current motion preference.
 * Re-renders when the OS setting changes OR when the html class changes.
 * Posture (decoy mode) is read from AuthModeContext so it re-renders on mode switch.
 */
export default function useMotionPreference(): MotionPreference {
  const { mode } = useAuthMode();
  const { systemInfo } = useAppState();

  // Snapshot initial state so the first render is already correct.
  const [reduced, setReduced] = useState<boolean>(
    () => resolvedWantsReduced()
  );

  useEffect(() => {
    if (typeof window === "undefined" || typeof document === "undefined") {
      return;
    }

    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");

    const recompute = () => {
      setReduced(resolvedWantsReduced());
    };

    // matchMedia listener — fires when OS setting changes.
    mq.addEventListener?.("change", recompute);

    // MutationObserver on html class list — fires when wc-no-motion is toggled
    // by AppShell (localStorage preference) or by any future caller.
    const observer =
      typeof MutationObserver === "undefined"
        ? undefined
        : new MutationObserver(recompute);

    observer?.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => {
      mq.removeEventListener?.("change", recompute);
      observer?.disconnect();
    };
  }, []);

  // Low-profile posture: decoy mode suppresses conspicuous motion.
  // Named by behavior (mode check), not by tell-words (DN-07).
  const isLowProfile = mode === "decoy";
  // The browser exposes logical cores but cannot reliably distinguish Windows
  // 10 from 11. The startup probe provides the canonical OS name and exact RAM.
  const systemNeedsReducedMotion = !userExplicitlyEnabledMotion() && shouldReduceMotionForSystem({
    osName: systemInfo?.osName,
    ramTotalGb: systemInfo?.ramTotalGb,
  });

  useEffect(() => {
    if (typeof document === "undefined") return;

    if (systemNeedsReducedMotion) {
      document.documentElement.classList.add("wc-no-motion");
    } else {
      // Restore the baseline class decision when a profile is not constrained
      // or the user has explicitly opted back into animation.
      applyMotionClass();
    }
  }, [systemNeedsReducedMotion]);

  return reduced || isLowProfile || systemNeedsReducedMotion ? "reduced" : "full";
}
