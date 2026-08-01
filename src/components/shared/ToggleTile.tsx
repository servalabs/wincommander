import { Icon, Spinner } from "@/components/ui/bp";
import type { IconName } from "@/components/ui/bp";
import { memo, useCallback } from "react";
import { playSound } from "../../utils/sound";
import { showInfo } from "../../utils/toast";
import WCSwitch from "./WCSwitch";
import "./ToggleTile.css";

/** Domain → CSS variable for icon colour */
const DOMAIN_COLOR: Record<string, string> = {
  privacy:  "var(--color-info)",
  network:  "var(--color-ok)",
  security: "var(--color-danger)",
  tweaks:   "var(--color-warn)",
  identity: "var(--color-accent)",
};

export interface ToggleTileProps {
  label: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  // silently accepted for API compatibility with UniversalToggle callsites:
  riskLevel?: "high" | "medium" | "low";
  requiresRestart?: boolean;
  severity?: "none" | "primary" | "success" | "warning" | "danger";
  disableIconSlash?: boolean;
  // visually active:
  disabled?: boolean;
  icon?: IconName;
  iconImage?: string;
  isAction?: boolean;
  actionType?: "run" | "open";
  loading?: boolean;
  className?: string;
  size?: "normal" | "compact";
  domain?: "privacy" | "security" | "network" | "tweaks" | "identity";
  onPulse?: () => void;
  riskFlags?: {
    needsAdmin: boolean;
    irreversible: boolean;
    reducesSecurity: boolean;
    defenderFlagged: boolean;
  };
  managedByOrg?: boolean;
  /** Labels of toggles that conflict with this one. No longer rendered as a
   *  passive badge here — ToggleSection now resolves conflicts at the point
   *  of activation via a confirm dialog. Kept for API compatibility with
   *  existing callsites. */
  conflictLabels?: string[];
}

// memo: ToggleTile is mounted dozens-to-hundreds of times per panel. Without
// memoizing, every appSettings change (any toggle anywhere, any background
// probe) re-renders every tile on screen, not just the one that changed.
function ToggleTile({
  label,
  description,
  checked,
  onChange,
  disabled = false,
  icon,
  iconImage,
  isAction = false,
  actionType = "run",
  loading = false,
  className = "",
  size = "normal",
  domain,
  onPulse,
  riskFlags,
  managedByOrg = false,
  // intentionally unused in tile layout:
  conflictLabels: _conflictLabels,
  riskLevel: _riskLevel,
  requiresRestart: _requiresRestart,
  severity: _severity,
  disableIconSlash: _disableIconSlash,
}: ToggleTileProps) {
  const iconColor = domain
    ? (DOMAIN_COLOR[domain] ?? "var(--color-accent)")
    : "var(--color-accent)";

  const isHardDisabled = disabled && !managedByOrg;
  // Single gate for every click/keyboard entry point: action tiles don't
  // toggle at all, managed-by-org tiles show an explainer instead, and
  // disabled/loading tiles are inert. Consolidates what used to be three
  // separately-duplicated conditions across the card, switch area, and
  // keyboard handlers.
  const isInteractive = !isAction && !disabled && !loading;

  const handleChange = useCallback(
    (next: boolean) => {
      playSound("toggle");
      onChange(next);
      onPulse?.();
    },
    [onChange, onPulse]
  );

  // Compact icons were 14px (illegible for browser/app logos); bumped to
  // match normal size so both read clearly in the dense 3-col grid.
  const iconSize = 18;

  const handleActivate = useCallback(() => {
    if (isAction) return;
    if (managedByOrg) {
      showInfo(
        "This setting is managed by your organization and cannot be changed."
      );
      return;
    }
    if (isInteractive) handleChange(!checked);
  }, [isAction, managedByOrg, isInteractive, handleChange, checked]);

  // WCSwitch fires onChange directly (it only self-gates on its own `disabled`
  // prop, which doesn't know about managedByOrg/isAction). Wrap it so the
  // switch short-circuits with the SAME gating order as the card body's
  // handleActivate — clicking the switch and clicking the rest of the tile
  // must behave identically instead of the switch bypassing the org-managed
  // lock. (Not just a call to handleActivate since the switch already knows
  // the intended next value; re-derived here for parity with handleActivate.)
  const handleSwitchChange = useCallback(
    (next: boolean) => {
      if (isAction) return;
      if (managedByOrg) {
        showInfo(
          "This setting is managed by your organization and cannot be changed."
        );
        return;
      }
      if (isInteractive) handleChange(next);
    },
    [isAction, managedByOrg, isInteractive, handleChange]
  );

  return (
    <div
      className={[
        "toggle-tile",
        checked ? "toggle-on" : "",
        size === "compact" ? "compact" : "",
        isHardDisabled ? "opacity-50 pointer-events-none" : "cursor-pointer",
        managedByOrg ? "opacity-50" : "",
        isAction ? "is-action" : "",
        className,
      ]
        .filter(Boolean)
        .join(" ")}
      onClick={handleActivate}
      data-needs-admin={riskFlags?.needsAdmin ? "true" : undefined}
      data-reduces-security={riskFlags?.reducesSecurity ? "true" : undefined}
      data-irreversible={riskFlags?.irreversible ? "true" : undefined}
      data-defender-flagged={riskFlags?.defenderFlagged ? "true" : undefined}
      title={
        riskFlags
          ? [
              riskFlags.defenderFlagged && "Defender / SmartScreen will flag this",
              riskFlags.irreversible && "Permanent — cannot be undone",
              riskFlags.reducesSecurity && "Weakens a Windows security feature",
              riskFlags.needsAdmin && "Requires administrator elevation",
            ]
              .filter(Boolean)
              .join(" • ") || undefined
          : undefined
      }
    >
      {/* Top row: icon + label on the left, switch/badge on the right */}
      <div className="tile-top">
        <div className="tile-icon-label">
          {iconImage ? (
            <img
              src={iconImage}
              alt=""
              width={iconSize}
              height={iconSize}
              style={{ flexShrink: 0, objectFit: "contain" }}
              loading="eager"
              onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none'; }}
            />
          ) : icon ? (
            <Icon
              icon={icon}
              size={iconSize}
              style={{ color: iconColor, flexShrink: 0 }}
            />
          ) : null}
          <span className="tile-label">
            {label}
            {managedByOrg && (
              <Icon
                icon="lock"
                size={10}
                style={{ color: "var(--color-text-muted)", marginLeft: 4 }}
                title="Managed by your organization"
              />
            )}
          </span>
        </div>

        <div className="tile-switch-area">
          {managedByOrg && (
            <span
              className="tile-managed-badge"
              title="This setting is enforced by your organization's admin config."
              onClick={(e) => e.stopPropagation()}
            >
              MANAGED
            </span>
          )}
          {isAction ? (
            <span className={`action-type-badge ${actionType} ${loading ? "is-running" : ""}`}>
              {loading ? <Spinner size={12} /> : null}
              {loading ? "RUNNING" : actionType.toUpperCase()}
            </span>
          ) : loading ? (
            <Spinner size={16} />
          ) : (
            <WCSwitch
              checked={checked}
              onChange={handleSwitchChange}
              size={size === "compact" ? "sm" : "md"}
              disabled={disabled}
              label={label}
            />
          )}
        </div>
      </div>

      {/* Bottom row: description */}
      <p className="tile-description">{description}</p>
    </div>
  );
}

export default memo(ToggleTile);
