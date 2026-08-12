export type Density = "guided" | "expert";

export type CapabilityBundle =
  | "essentials"
  | "privacy"
  | "network"
  | "monitoring"
  | "safeguards";

export const ALWAYS_CAPABILITY: CapabilityBundle = "essentials";
