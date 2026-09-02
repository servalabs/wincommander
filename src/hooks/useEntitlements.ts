// src/hooks/useEntitlements.ts
//
// ═══════════════════════════════════════════════════════════════════════
// ENTITLEMENTS — Reactive paid-feature gate
// ═══════════════════════════════════════════════════════════════════════
//
// Single source of truth for "does this user have paid features unlocked?"
// Everything paid-tier in the UI consults this hook. Derived from the
// license JWT's `features` vector:
//   - "paid" → paid licence or trial
//
// Anything else (empty vector, no license) → free tier only.
//
// Usage:
//   const { hasPaid, canUse } = useEntitlements();
//   if (toggle.tier === "paid" && !canUse(toggle.tier)) → show <LockedToggle>
//
// While the license is loading, canUse() is optimistic (returns true for
// every tier) so paid panels don't flash a locked state on app start.
// hasPaid stays false during loading so any logic that explicitly needs
// "I know this user is entitled" still has a defensive default. The
// backend's require_paid() check is the actual source of truth at
// command-dispatch time, so the optimistic UI gate can't be exploited.

import { useMemo } from "react";
import type { Tier } from "../types/toggles";
import { useLicenseQuery } from "./queries/useLicenseQuery";
import { useAppState } from "../context/AppContext";

export interface Entitlements {
  /** True if the user has any paid entitlement (including 16-day trial). */
  hasPaid: boolean;

  /** True only while the signed licence covers downloading/installing a Pro build. */
  canUpdatePro: boolean;

  /** True if the user is currently in a free trial (subset of hasPaid). */
  isTrial: boolean;

  /**
   * True when the active licence's features array contains "advanced"
   * -- the marker for evidence-collection mode. UI consumers should use
   * this to hide every erase/clear button, gate cleanup-acquire flows on
   * an active case session, and show the investigator-mode banner.
   *
   * Strict on the literal "advanced" string -- not granted by the
   * ordinary "paid" feature. Mirrors the Rust-side
   * license::is_advanced_mode() contract exactly.
   */
  isInvestigator: boolean;

  /**
   * True when the active licence's features array contains "advanced",
   * regardless of the Settings consent toggle. Lets UI distinguish
   * "no licence" from "licensed but consent not yet given" so it can
   * show the right call-to-action instead of a single generic locked state.
   */
  investigatorEligible: boolean;

  /** Returns true if the user can use a feature of the given tier. */
  canUse: (tier: Tier) => boolean;

  /** Loading state from the license query. */
  isLoading: boolean;
}

export default function useEntitlements(): Entitlements {
  const { data: license, isLoading } = useLicenseQuery();
  const { appSettings } = useAppState();
  // Settings "advanced mode" master switch. Mirrors the Rust-side
  // consent gate in license::is_advanced_mode — the kill-switch only
  // arms when BOTH the licence claim AND this toggle are on. Without
  // composing it here, the cleanup banner + investigator panel showed
  // up purely on the licence even when the user had turned the toggle
  // OFF in Settings, drifting out of sync with the backend's actual
  // behaviour.
  const investigatorToggle = appSettings?.ideal?.identity?.advancedToolsEnabled === true;

  const hasPaid = useMemo(() => {
    if (!license || !license.valid) return false;
    const features = license.features ?? [];
    return features.includes("paid");
  }, [license]);

  const isTrial = !!license?.trial_active && hasPaid;
  // Feature access and update access are deliberately separate. A perpetual
  // licence may keep using an installed Pro build after update coverage ends;
  // Rust rechecks this signed flag before any new Pro binary is installed.
  const canUpdatePro = hasPaid && license?.update_entitled === true;

  // Investigator mode -- requires BOTH (a) a valid licence whose features
  // include the literal "advanced" string AND (b) the user's
  // explicit Settings toggle. Either alone is insufficient.
  //
  // The licence half is strict on "advanced" — not granted by "paid"
  // and is never implied by a trial.
  //
  // The toggle half lets the user step in/out of cleanup mode without
  // changing licences. With it off, the Investigator panel hides from
  // the sidebar, the Cleanup-panel banner is suppressed, and the
  // Rust dispatch refusal (license.rs is_advanced_mode) also stays
  // disarmed so the user's everyday Clear-* / Erase-* commands work.
  const investigatorEligible = useMemo(() => {
    if (!license || !license.valid) return false;
    const features = license.features ?? [];
    return features.includes("advanced");
  }, [license]);

  const isInvestigator = useMemo(() => {
    if (!investigatorEligible) return false;
    return investigatorToggle;
  }, [investigatorEligible, investigatorToggle]);

  const canUse = useMemo(() => {
    return (tier: Tier): boolean => {
      if (tier === "free") return true;
      // Optimistic during the first license-status fetch — avoids the
      // visible "lock everything for ~300ms then unlock" flicker on app
      // start. Backend require_paid() still enforces at dispatch time.
      if (isLoading) return true;
      return hasPaid;
    };
  }, [hasPaid, isLoading]);

  return { hasPaid, canUpdatePro, isTrial, isInvestigator, investigatorEligible, canUse, isLoading };
}
