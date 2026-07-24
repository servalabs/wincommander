import type { ReactNode } from "react";
import "./SelectCard.css";

interface SelectCardProps {
  title: string;
  subtitle?: string;
  /** Body copy (clamped to 3 lines so cards in a grid stay even-height). */
  blurb?: string;
  /** Top-right tag — pass a <Badge> (free/pro/locked, etc.). */
  badge?: ReactNode;
  active: boolean;
  locked?: boolean;
  onSelect: () => void;
  /** Bottom-pinned footer: status pill + optional action button. */
  footer?: ReactNode;
  className?: string;
}

/**
 * SelectCard — the dns-card, generalized (picked 2026-06).
 * A single-select status card: header (title + tier badge), subtitle, clamped
 * blurb, bottom-pinned footer. Active = accent left-bar + success border +
 * success-dim fill. Drop into an `auto-fit minmax()` grid for pick-one-of-N.
 */
export default function SelectCard({
  title,
  subtitle,
  blurb,
  badge,
  active,
  locked = false,
  onSelect,
  footer,
  className = "",
}: SelectCardProps) {
  const cls = [
    "wc-selectcard",
    active ? "is-active" : "",
    locked ? "is-locked" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={cls}
      role="button"
      tabIndex={locked ? -1 : 0}
      aria-pressed={active}
      onClick={() => {
        if (!locked) onSelect();
      }}
      onKeyDown={(e) => {
        if ((e.key === "Enter" || e.key === " ") && !locked) {
          e.preventDefault();
          onSelect();
        }
      }}
    >
      <div className="wc-selectcard__head">
        <span className="wc-selectcard__title">{title}</span>
        {badge}
      </div>
      {subtitle ? <div className="wc-selectcard__sub">{subtitle}</div> : null}
      {blurb ? <div className="wc-selectcard__blurb">{blurb}</div> : null}
      {footer ? <div className="wc-selectcard__foot">{footer}</div> : null}
    </div>
  );
}
