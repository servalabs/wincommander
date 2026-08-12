import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * `cn` — merge conditional class names and de-dupe conflicting Tailwind
 * utilities (the standard shadcn/ui helper). Used by the V2 component kit
 * in `src/components/ui/`.
 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
