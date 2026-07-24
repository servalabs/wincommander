import { Icon, type IconName } from "@/components/ui/bp";
import type { ReactNode } from "react";
import "./Badge.css";

export type BadgeTone =
  | "neutral"
  | "accent"
  | "success"
  | "warning"
  | "danger"
  | "info";

interface BadgeProps {
  /** Colour family. Maps tier (free→success, pro→accent, locked→neutral) and
   *  severity (critical→danger, high→warning) onto one token-driven scale. */
  tone?: BadgeTone;
  icon?: IconName;
  className?: string;
  children: ReactNode;
}

/**
 * Badge — the universal status/tier/severity chip (2px corners, mono-uppercase).
 * Replaces the 35+ bespoke `*-badge` / `*-pill` classes scattered across panels.
 */
export default function Badge({ tone = "neutral", icon, className = "", children }: BadgeProps) {
  return (
    <span className={`wc-badge wc-badge--${tone} ${className}`.trim()}>
      {icon ? <Icon icon={icon} size={9} /> : null}
      {children}
    </span>
  );
}
