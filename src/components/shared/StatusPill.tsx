import { Icon, type IconName } from "@/components/ui/bp";
import type { ReactNode } from "react";
import "./StatusPill.css";

export type StatusPillTone = "neutral" | "success" | "warning" | "danger" | "info";

interface StatusPillProps {
  tone?: StatusPillTone;
  icon?: IconName;
  className?: string;
  children: ReactNode;
}

export default function StatusPill({
  tone = "neutral",
  icon,
  className = "",
  children,
}: StatusPillProps) {
  return (
    <span className={`status-pill status-pill--${tone} ${className}`.trim()}>
      {icon ? <Icon icon={icon} size={10} /> : null}
      <span>{children}</span>
    </span>
  );
}
