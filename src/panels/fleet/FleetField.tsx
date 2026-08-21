import type { ReactNode } from "react";
import { Label } from "@/components/ui/label";

export default function FleetField({ label, htmlFor, hint, compact, children }: {
  label: string;
  htmlFor?: string;
  hint?: string;
  compact?: boolean;
  children: ReactNode;
}) {
  return (
    <label className={`fleet-field${compact ? " is-compact" : ""}`} title={compact ? hint : undefined}>
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
      {hint && !compact && <span className="fleet-field-hint">{hint}</span>}
    </label>
  );
}
