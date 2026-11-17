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
