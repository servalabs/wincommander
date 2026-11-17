// Operator-facing wording for Fleet connection failures. The backend enforces
// the HTTPS rule (fleet_agent.rs require_secure_fleet_origin); this module only
// turns its rejections into actionable text and previews them while typing.

export const FLEET_HTTPS_HINT =
  "Fleet requires an https:// address. Change http:// to https:// and use the server's HTTPS port " +
  "(for example https://fleet.example.com or https://host.tailnet.ts.net:9443). Plain http:// is never accepted.";

export const FLEET_ADDRESS_HINT =
  "Enter the full Fleet address starting with https:// (for example https://fleet.example.com:9443), with no path.";

/** Hint for the address as typed; null when nothing is wrong yet. */
export function fleetAddressWarning(raw: string): string | null {
  const value = raw.trim();
  if (!value) return null;
  if (/^http:/i.test(value)) return FLEET_HTTPS_HINT;
  if (!/^https:\/\//i.test(value)) return FLEET_ADDRESS_HINT;
  return null;
}

/** Map a backend connect failure to an actionable message. */
export function describeFleetConnectError(message: string): string {
  if (/requires an https:\/\/ address|requires an HTTPS server/i.test(message)) return FLEET_HTTPS_HINT;
  if (/valid http\(s\) origin/i.test(message)) return FLEET_ADDRESS_HINT;
  return message;
}

const TRUST_BRIDGE_HINTS: Record<string, string> = {
  identity_change_unconfirmed:
    "The Fleet server's enrollment differs from the one this device's service trusts, and the change was not confirmed.",
  replacement_refused:
    "The Fleet server's enrollment belongs to a different device, key or organization than the service trusts, so it was not replaced.",
  binding_changed:
    "The local service holds a different enrollment than this device's Fleet agent, so its Shield policy cannot be verified.",
  generation_must_change:
    "The local service already holds this enrollment; no replacement was needed.",
  invalid_proof:
    "The service could not verify the agent's enrollment proof for the pending attempt.",
  hybrid_replacement_required:
    "This machine uses hybrid enrollment, which cannot be replaced from this panel.",
  service_rejected: "The local service refused to record this device's enrollment.",
  proof_timeout:
    "The Fleet agent did not finish enrolling in time for the service to record its trust.",
  other: "The service could not record this device's enrollment.",
};

/** Closed-vocabulary hint for a failed service trust bridge; null for no failure. */
export function describeTrustBridgeError(code: string | null | undefined): string | null {
  if (!code) return null;
  return TRUST_BRIDGE_HINTS[code] ?? TRUST_BRIDGE_HINTS.other;
}

// Codes where the service's stored trust disagrees with the agent's enrollment,
// which the explicit machine-trust repair can fix. Other codes (a timeout, an
// unconfirmed identity change on another server, hybrid trust) are shown as
// information only.
const REPAIRABLE_TRUST_CODES = new Set([
  "identity_change_unconfirmed",
  "binding_changed",
  "invalid_proof",
]);

export function trustBridgeNeedsRepair(code: string | null | undefined): boolean {
  return !!code && REPAIRABLE_TRUST_CODES.has(code);
}

const EPOCH_ADOPTION_HINTS: Record<string, string> = {
  epoch_generation_unconfirmed:
    "Fleet policy from the new server needs adoption, but this device's service has not confirmed the new enrollment yet. Repair machine trust, then the policy applies automatically.",
  epoch_below_service_floor:
    "The Fleet policy from the new server is older than the policy this device's service already holds for it, so it was not adopted.",
  epoch_rollback_refused:
    "An older Fleet policy than the one already applied was refused (rollback protection).",
};

/** Closed-vocabulary hint for why a signed Fleet policy epoch was not adopted. */
export function describeEpochAdoptionIssue(code: string | null | undefined): string | null {
  if (!code) return null;
  return EPOCH_ADOPTION_HINTS[code] ?? "A signed Fleet policy could not be adopted.";
}

/** Only an unconfirmed new enrollment is cleared by the trust repair. */
export function epochIssueNeedsRepair(code: string | null | undefined): boolean {
  return code === "epoch_generation_unconfirmed";
}
