// src/panels/vault/MountScopeSelector.tsx
//
// Lets the user choose who can see a mounted vault: everyone signed into
// this PC (machine scope) or only the signed-in user (per-user scope) — see
// EncVolMountScope in EncVolFormatSdk.h for why these are two genuinely
// different engine resources, not a cosmetic label. Admin-lockable: a Fleet
// policy can pin the value via policy.lockedPaths ("vault.mountScope"), in
// which case this renders disabled with a note instead of hiding the control
// (same pattern as the RDP Idle card's "MANAGED BY ORG" lock).
import { Icon } from "@/components/ui/bp";
import type { MountScopePreference } from "./mountScope";
import "./MountScopeSelector.css";

interface MountScopeSelectorProps {
  id?: string;
  value: MountScopePreference;
  onChange: (value: MountScopePreference) => void;
  /** True when a Fleet policy has pinned this setting. The control still
   *  shows the pinned value — it just can't be changed locally. */
  locked?: boolean;
  disabled?: boolean;
}

const OPTIONS: { value: MountScopePreference; label: string; description: string }[] = [
  { value: "auto", label: "Auto (recommended)", description: "Per-user on shared or server PCs, per-machine on a personal PC." },
  { value: "machine", label: "Per machine", description: "Anyone signed into this PC can see the mounted drive." },
  { value: "per-user", label: "Per user", description: "Only you can see it, even if someone else is signed in at the same time." },
];

export default function MountScopeSelector({ id, value, onChange, locked, disabled }: MountScopeSelectorProps) {
  const isDisabled = Boolean(locked || disabled);

  return (
    <div id={id} className="mount-scope-selector">
      <div className="mount-scope-options" role="radiogroup" aria-label="Mount scope">
        {OPTIONS.map((opt) => (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={value === opt.value}
            disabled={isDisabled}
            className={`mount-scope-option${value === opt.value ? " is-selected" : ""}`}
            onClick={() => !isDisabled && onChange(opt.value)}
          >
            <span className="mount-scope-option-label">{opt.label}</span>
            <span className="mount-scope-option-desc">{opt.description}</span>
          </button>
        ))}
      </div>
      {locked && (
        <div className="mount-scope-locked-note">
          <Icon icon="lock" size={10} />
          <span>Set by your administrator.</span>
        </div>
      )}
    </div>
  );
}
