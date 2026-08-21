import * as React from "react";
import { cn } from "../../lib/utils";
import { Icon, type IconName } from "./icon";

export function EmptyState({ icon, title, description, action, children, className }: {
  icon?: IconName | React.ReactElement; title?: React.ReactNode; description?: React.ReactNode;
  action?: React.ReactNode; children?: React.ReactNode; className?: string;
}) {
  return <div className={cn("flex flex-col items-center justify-center gap-3 py-12 text-center", className)}>
    {icon != null && (typeof icon === "string" ? <div className="grid h-12 w-12 place-items-center rounded-full border border-[var(--border)] bg-[var(--surface-2)] text-[var(--text-mute)]"><Icon icon={icon} size={22} /></div> : icon)}
    {title != null && <div className="text-[15px] font-semibold text-[var(--text)]">{title}</div>}
    {description != null && <div className="max-w-[320px] text-[13px] text-[var(--text-dim)]">{description}</div>}
    {action}{children}
  </div>;
}
