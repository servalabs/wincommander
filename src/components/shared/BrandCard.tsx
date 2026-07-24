import { Spinner } from "@/components/ui/bp";
import type { ReactNode } from "react";
import WCSwitch from "./WCSwitch";
import "./BrandCard.css";

interface BrandCardProps {
  /** Top-strip content: logo image(s) or a short mono label. */
  logos?: ReactNode;
  /** Right-aligned meta on the top strip, e.g. "12,480 domains". */
  meta?: ReactNode;
  title: string;
  description?: string;
  /** On/applied state. */
  active: boolean;
  onToggle: (next: boolean) => void;
  loading?: boolean;
  disabled?: boolean;
  /** Active visual: `accent` (cyan — default, app house language) or
   *  `success` (green "protected" — e.g. the hosts blocklist). */
  activeTone?: "accent" | "success";
  className?: string;
}

/**
 * BrandCard — the hosts-blocklist card, generalized (picked 2026-06).
 * A logo/title top strip + a click-anywhere body that toggles, with a status
 * chip and WCSwitch. Use for any catalog of toggleable named things (apps,
 * integrations, modules, blocklists).
 */
export default function BrandCard({
  logos,
  meta,
  title,
  description,
  active,
  onToggle,
  loading = false,
  disabled = false,
  activeTone = "accent",
  className = "",
}: BrandCardProps) {
  const cls = [
    "wc-brandcard",
    active ? "is-active" : "",
    `wc-brandcard--${activeTone}`,
    disabled || loading ? "is-busy" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={cls}>
      <div className="wc-brandcard__top">
        <span className="wc-brandcard__logos">{logos ?? title}</span>
        {meta ? <span className="wc-brandcard__meta">{meta}</span> : null}
      </div>
      <div
        className="wc-brandcard__body"
        role="button"
        tabIndex={disabled ? -1 : 0}
        onClick={() => {
          if (!disabled && !loading) onToggle(!active);
        }}
        onKeyDown={(e) => {
          if ((e.key === "Enter" || e.key === " ") && !disabled && !loading) {
            e.preventDefault();
            onToggle(!active);
          }
        }}
      >
        <div className="wc-brandcard__text">
          <div className="wc-brandcard__title">{title}</div>
          {description ? <div className="wc-brandcard__desc">{description}</div> : null}
        </div>
        <div className="wc-brandcard__action">
          {loading ? <Spinner size={16} /> : null}
          <span className={`status-text ${active ? "active" : "standby"}`}>
            {active ? "Active" : "Standby"}
          </span>
          <WCSwitch checked={active} onChange={onToggle} disabled={disabled || loading} label={title} />
        </div>
      </div>
    </div>
  );
}
