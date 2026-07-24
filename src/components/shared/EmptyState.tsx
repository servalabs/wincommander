import { Icon, type IconName } from "@/components/ui/bp";
import type { ReactNode } from "react";
import "./EmptyState.css";

interface EmptyStateProps {
  /** Icon for the rich variant (ignored when `compact`). */
  icon?: IconName;
  title: string;
  hint?: string;
  /** Minimal single muted line — for small cards/sub-lists where an icon is overkill. */
  compact?: boolean;
  /** Optional action (e.g. a "Run scan" button) shown under the hint. */
  action?: ReactNode;
  className?: string;
}

/**
 * EmptyState — the universal "nothing here yet" treatment.
 * Replaces the 6 bespoke empty-state classes + raw inline strings across panels.
 */
export default function EmptyState({ icon, title, hint, compact = false, action, className = "" }: EmptyStateProps) {
  if (compact) {
    return <div className={`wc-empty wc-empty--compact ${className}`.trim()}>{title}</div>;
  }
  return (
    <div className={`wc-empty ${className}`.trim()}>
      {icon ? (
        <span className="wc-empty__icon">
          <Icon icon={icon} size={34} />
        </span>
      ) : null}
      <div className="wc-empty__title">{title}</div>
      {hint ? <div className="wc-empty__hint">{hint}</div> : null}
      {action ? <div className="wc-empty__action">{action}</div> : null}
    </div>
  );
}
