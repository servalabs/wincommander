import type { ReactNode } from "react";
import { Label } from "@/components/ui/label";

export default function SecurityField({ label, htmlFor, hint, children }: {
  label: string;
  htmlFor?: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="fleet-field">
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
      {hint && <span className="fleet-field-hint">{hint}</span>}
    </label>
  );
}
