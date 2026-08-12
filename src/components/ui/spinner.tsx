import * as React from "react";
import { Loader2 } from "lucide-react";
import { cn } from "../../lib/utils";

/** Lucide-backed spinner replacing Blueprint's <Spinner size=.. />. */
export function Spinner({
  size = 16,
  className,
  ...props
}: { size?: number; className?: string } & Omit<React.ComponentProps<typeof Loader2>, "size">) {
  return <Loader2 size={size} className={cn("animate-spin", className)} {...props} />;
}

export default Spinner;
