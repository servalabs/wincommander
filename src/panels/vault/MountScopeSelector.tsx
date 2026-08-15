// src/panels/vault/MountScopeSelector.tsx
//
// Lets the user choose who can see a mounted vault: everyone signed into
// this PC (machine scope) or only the signed-in user (per-user scope) — see
// EncVolMountScope in EncVolFormatSdk.h for why these are two genuinely
// different engine resources, not a cosmetic label. Admin-lockable two ways,
// checked here directly (rather than trusting a caller-passed `locked` prop)
// so any future caller gets the lock for free, matching how ToggleSection/
// RdpIdleCard derive their own lock state instead of taking it as a prop:
//   - a Fleet policy pin via policy.lockedPaths ("vault.mountScope") — needs
//     a connected admin server to populate;
//   - policy.pinnedMountScope — set locally by the single administrator on a
//     standalone (no Fleet server) box, where lockedPaths never gets written.
// Either way this renders disabled with a note instead of hiding the control
// (same pattern as the RDP Idle card's "MANAGED BY ORG" lock), and shows the
// pinned value rather than the caller's `value` so a stale local preference
// never displays as selected while a pin overrides it.
import { Icon } from "@/components/ui/bp";
import { useAppState } from "../../context/AppContext";
import type { MountScopePreference } from "./mountScope";
import "./MountScopeSelector.css";

interface MountScopeSelectorProps {
  id?: string;
  value: MountScopePreference;
  onChange: (value: MountScopePreference) => void;
  /** Forces the locked state on top of whatever policy already derives —
   *  kept for callers that already know they're locked (e.g. a disabled
   *  wizard step); not required for the Fleet/admin-pin cases below, which
   *  this component now checks itself. */
  locked?: boolean;
  disabled?: boolean;
}

const OPTIONS: { value: MountScopePreference; label: string; description: string }[] = [
  { value: "auto", label: "Auto (recommended)", description: "Per-user on shared or server PCs, per-machine on a personal PC." },
  { value: "machine", label: "Per machine", description: "Anyone signed into this PC can see the mounted drive." },
  { value: "per-user", label: "Per user", description: "Only you can see it, even if someone else is signed in at the same time." },
];

export default function MountScopeSelector({ id, value, onChange, locked, disabled }: MountScopeSelectorProps) {
  const { appSettings } = useAppState();

  const lockedPaths = appSettings?.policy?.lockedPaths ?? [];
  // Tolerates both the bare fleet dot-path convention ("vault.mountScope")
  // and this app's "app."-prefixed convention (see CreateVolumeWizard.tsx /
  // MetricAlertRow.tsx), whichever the connected server actually publishes.
  const fleetLocked = lockedPaths.some(
    (p) => p.trim().length > 0 && ("vault.mountScope".startsWith(p) || "app.vault.mountScope".startsWith(p))
  );
  const pinnedMountScope = appSettings?.policy?.pinnedMountScope ?? null;
  const policyLocked = fleetLocked || pinnedMountScope !== null;
  const isDisabled = Boolean(locked || disabled || policyLocked);
  // Distinct from isDisabled: a plain `disabled` prop (e.g. a wizard step
  // that isn't reachable yet) isn't an admin action and shouldn't claim one.
  const showLockedNote = Boolean(locked || policyLocked);
  // While pinned, the pin is authoritative over whatever the caller's own
  // (possibly stale, possibly pre-pin) preference says.
  const effectiveValue = pinnedMountScope ?? value;

  return (
    <div id={id} className="mount-scope-selector">
      <div className="mount-scope-options" role="radiogroup" aria-label="Mount scope">
        {OPTIONS.map((opt) => (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={effectiveValue === opt.value}
            disabled={isDisabled}
            className={`mount-scope-option${effectiveValue === opt.value ? " is-selected" : ""}`}
            onClick={() => !isDisabled && onChange(opt.value)}
          >
            <span className="mount-scope-option-label">{opt.label}</span>
            <span className="mount-scope-option-desc">{opt.description}</span>
          </button>
        ))}
      </div>
      {showLockedNote && (
        <div className="mount-scope-locked-note">
          <Icon icon="lock" size={10} />
          <span>Set by your administrator.</span>
        </div>
      )}
    </div>
  );
}
