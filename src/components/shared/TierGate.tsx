// src/components/shared/TierGate.tsx
//
// ═══════════════════════════════════════════════════════════════════════
// TIER GATE — Generic wrapper for any paid-feature UI
// ═══════════════════════════════════════════════════════════════════════
//
// Use this when you want to gate an arbitrary block of UI (a section, an
// action button, a dialog content) behind a paid entitlement. For toggle
// rows specifically, ToggleSection already inserts <LockedToggle> so you
// don't need TierGate there.
//
// Usage:
//   <TierGate tier="paid" featureLabel="Vault create volume">
//     <CreateVolumeWizard />
//   </TierGate>
//
// Behaviour:
//   - tier="free" → always renders children
//   - tier="paid" + entitled → renders children
//   - tier="paid" + not entitled → renders the locked fallback (a small
//     button with lock icon that opens the LicenseGate dialog on click)
//
// If you want a different fallback (e.g. inline message instead of a
// button), pass it via the `fallback` prop.

import { type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import type { Tier } from "../../types/toggles";
import useEntitlements from "../../hooks/useEntitlements";

export interface TierGateProps {
  tier: Tier;
  children: ReactNode;
  /** Optional feature label surfaced in the LicenseGate dialog header. */
  featureLabel?: string;
  /** Optional custom fallback rendered when tier="paid" and not entitled. */
  fallback?: ReactNode;
}

export default function TierGate({ tier, children, featureLabel, fallback }: TierGateProps) {
  const { canUse, isLoading } = useEntitlements();

  const openLicenseGate = () => {
    window.dispatchEvent(
      new CustomEvent("license-gate-open", { detail: { tab: "buy", featureLabel } })
    );
  };

  if (isLoading) {
    // Don't flash the locked state while license is loading — render nothing.
    return null;
  }

  if (canUse(tier)) {
    return <>{children}</>;
  }

  if (fallback !== undefined) {
    return <>{fallback}</>;
  }

  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={openLicenseGate}
      className="tier-gate-locked"
      style={{
        color: "var(--color-accent)",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        textTransform: "uppercase",
        letterSpacing: 1,
      }}
    >
      <Icon icon="lock" size={11} style={{ marginRight: 6 }} />
      Unlock with WinCommander Pro
    </Button>
  );
}
