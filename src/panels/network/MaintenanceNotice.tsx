// Small shared presentation pieces for the network maintenance cards.
// v2 kit only — no bp imports here.
import type { ReactNode } from "react";
import { Icon, type IconName } from "../../components/ui/icon";

type NoticeTone = "primary" | "success" | "warning" | "danger";

const TONE_COLOR: Record<NoticeTone, string> = {
  primary: "var(--accent)",
  success: "var(--ok)",
  warning: "var(--warn)",
  danger: "var(--danger)",
};

const TONE_ICON: Record<NoticeTone, IconName> = {
  primary: "info-sign",
  success: "tick-circle",
  warning: "warning-sign",
  danger: "error",
};

/** Headline + explanation banner. The headline is the reading of the result,
 *  the body is what to do about it — the pair is the whole point of these
 *  diagnostics cards, so it gets a dedicated component rather than a bare <p>. */
export function MaintenanceNotice({
  tone,
  headline,
  children,
  action,
}: {
  tone: NoticeTone;
  headline: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  const color = TONE_COLOR[tone];
  return (
    <div
      className="flex gap-2.5 rounded-[var(--r)] border-l-2 px-3 py-2.5"
      style={{
        borderLeftColor: color,
        background: `color-mix(in srgb, ${color} 8%, transparent)`,
      }}
    >
      <Icon icon={TONE_ICON[tone]} size={15} color={color} className="mt-[1px] shrink-0" />
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-semibold text-[var(--text)]">{headline}</div>
        {children ? (
          <div className="mt-1 text-[12px] leading-relaxed text-[var(--text-dim)]">{children}</div>
        ) : null}
        {action ? <div className="mt-2 flex flex-wrap gap-2">{action}</div> : null}
      </div>
    </div>
  );
}

/** Placeholder rows while a shell-out is in flight. Keeps the card height
 *  roughly stable instead of collapsing and re-expanding. */
export function TableSkeleton({ rows = 5, label }: { rows?: number; label: string }) {
  return (
    <div className="flex flex-col gap-1.5" role="status" aria-label={label}>
      <span className="text-[12px] text-[var(--text-mute)]">{label}</span>
      {Array.from({ length: rows }, (_, i) => (
        <div
          key={i}
          className="h-7 animate-pulse rounded-[var(--r-sm)] bg-[var(--surface-2)]"
          style={{ animationDelay: `${i * 70}ms` }}
        />
      ))}
    </div>
  );
}

/** Click-to-sort table header cell. */
export function SortHeader({
  label,
  active,
  direction,
  onClick,
  info,
  className,
}: {
  label: string;
  active: boolean;
  direction: "asc" | "desc";
  onClick: () => void;
  info?: ReactNode;
  className?: string;
}) {
  return (
    <th scope="col" className={className}>
      <span className="inline-flex items-center gap-1">
        <button
          type="button"
          onClick={onClick}
          className={`inline-flex items-center gap-1 uppercase tracking-wider transition-colors hover:text-[var(--text)] ${
            active ? "text-[var(--accent)]" : ""
          }`}
        >
          {label}
          <Icon
            icon={active && direction === "desc" ? "chevron-up" : "chevron-down"}
            size={11}
            className={active ? "" : "opacity-30"}
          />
        </button>
        {info}
      </span>
    </th>
  );
}
