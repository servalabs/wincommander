// LockedToggle — tile-layout variant shown when a paid feature is un-entitled.
// Same card structure as ToggleTile; no switch — shows a lock PRO badge instead.
// Clicking anywhere opens the LicenseGate purchase dialog.
import { Icon } from "@/components/ui/bp";
import type { IconName } from "@/components/ui/bp";
import Badge from "./Badge";
import "./ToggleTile.css";

export interface LockedToggleProps {
  label: string;
  description: string;
  icon?: IconName;
  size?: "normal" | "compact";
  domain?: string;
}

const DOMAIN_COLOR: Record<string, string> = {
  privacy:  "var(--color-info)",
  network:  "var(--color-ok)",
  security: "var(--color-danger)",
  tweaks:   "var(--color-warn)",
  identity: "var(--color-accent)",
};

export default function LockedToggle({
  label,
  description,
  icon,
  size = "compact",
  domain,
}: LockedToggleProps) {
  const openLicenseGate = () => {
    window.dispatchEvent(
      new CustomEvent("license-gate-open", {
        detail: { tab: "buy", featureLabel: label },
      })
    );
  };

  const iconColor = domain
    ? (DOMAIN_COLOR[domain] ?? "var(--color-text-muted)")
    : "var(--color-text-muted)";

  return (
    <div
      className={[
        "toggle-tile",
        // is-action: opts the tile out of ToggleTile.css's OFF-state text
        // muting (that rule targets :not(.toggle-on):not(.is-action)) so a
        // locked tile isn't dimmed twice — once by that rule, once by being
        // visually "off" — which read noticeably darker than sibling OFF
        // tiles. The PRO badge/lock affordance is unaffected.
        "is-action",
        size === "compact" ? "compact" : "",
        "cursor-pointer",
      ]
        .filter(Boolean)
        .join(" ")}
      onClick={openLicenseGate}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          openLicenseGate();
        }
      }}
    >
      {/* Top row */}
      <div className="tile-top">
        <div className="tile-icon-label">
          {icon && (
            <Icon
              icon={icon}
              size={size === "compact" ? 14 : 18}
              style={{ color: iconColor, flexShrink: 0 }}
            />
          )}
          <span className="tile-label">{label}</span>
        </div>

        {/* PRO badge in switch slot */}
        <Badge tone="accent" icon="lock" className="shrink-0">PRO</Badge>
      </div>

      {/* Bottom row */}
      <p className="tile-description">{description}</p>
    </div>
  );
}
