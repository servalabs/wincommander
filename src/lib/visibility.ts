import type { CapabilityBundle, Density } from "../types/persona";

export interface Visibility {
  minDensity?: Density;
  capability?: CapabilityBundle[];
  presentation?: "preset" | "detail" | "both";
  dependency?: string;
  tier?: "free" | "paid";
}

export interface VisibilityCtx {
  density: Density;
  profiles: Set<CapabilityBundle>;
  dependencies: Set<string>;
}

const DENSITY_ORDER: Density[] = ["guided", "expert"];

export function isVisible(visibility: Visibility | undefined, ctx: VisibilityCtx): boolean {
  if (!visibility) {
    return true;
  }

  if (
    visibility.minDensity &&
    DENSITY_ORDER.indexOf(ctx.density) < DENSITY_ORDER.indexOf(visibility.minDensity)
  ) {
    return false;
  }

  if (
    visibility.capability &&
    visibility.capability.length > 0 &&
    !visibility.capability.some(
      (capability) => capability === "essentials" || ctx.profiles.has(capability),
    )
  ) {
    return false;
  }

  if (visibility.dependency && !ctx.dependencies.has(visibility.dependency.toLowerCase())) {
    return false;
  }

  return true;
}
