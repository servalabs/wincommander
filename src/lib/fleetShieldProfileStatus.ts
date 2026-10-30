export type FleetShieldProfileStatus = "verified" | "effective_config_unavailable" | "unverified";

export function fleetShieldProfileStatus(value: unknown): FleetShieldProfileStatus {
  if (!value || typeof value !== "object" || !("trustBound" in value) || value.trustBound !== true) return "unverified";
  if (!("shieldPolicyStatus" in value)) return "unverified";
  return value.shieldPolicyStatus === "verified" || value.shieldPolicyStatus === "effective_config_unavailable"
    ? value.shieldPolicyStatus : "unverified";
}
