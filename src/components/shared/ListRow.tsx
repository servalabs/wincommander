import { Icon, type IconName } from "@/components/ui/bp";
import type { ReactNode } from "react";
import "./ListRow.css";

export type ListRowDensity = "comfortable" | "compact";
export type ListRowSeverity = "danger" | "warning" | "info" | "success";

interface ListRowProps {
  /** Leading icon tile (optional). */
  icon?: IconName;
  title: ReactNode;
  /** Secondary muted line under the title (path, size, count…). */
  meta?: ReactNode;
  /** Right-aligned control cluster (badge, switch, button…). */
  trailing?: ReactNode;
  /** Row height/padding. Density stays case-by-case per the data shown. */
  density?: ListRowDensity;
  /** Left-edge severity channel colour. */
  severity?: ListRowSeverity;
  onClick?: () => void;
  className?: string;
}

/**
 * ListRow — the universal "leading icon · title · meta · trailing control" row.
 * One component, two densities (comfortable / compact) so callers pick the
 * weight that fits their data without re-rolling row markup each time.
 */
export default function ListRow({
  icon,
  title,
  meta,
  trailing,
  density = "comfortable",
  severity,
  onClick,
  className = "",
}: ListRowProps) {
  const cls = [
    "wc-listrow",
    `wc-listrow--${density}`,
    severity ? `wc-listrow--sev-${severity}` : "",
    onClick ? "wc-listrow--clickable" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={cls}
      onClick={onClick}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
      onKeyDown={
        onClick
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick();
              }
            }
          : undefined
      }
    >
      {icon ? (
        <span className="wc-listrow__lead">
          <Icon icon={icon} size={density === "compact" ? 12 : 14} />
        </span>
      ) : null}
      <div className="wc-listrow__main">
        <div className="wc-listrow__title">{title}</div>
        {meta ? <div className="wc-listrow__meta">{meta}</div> : null}
      </div>
      {trailing ? <div className="wc-listrow__trailing">{trailing}</div> : null}
    </div>
  );
}
