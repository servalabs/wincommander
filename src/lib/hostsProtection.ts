export interface HostsVerification {
  state: "off" | "verified" | "partial" | "attention";
  message: string;
  matched: number;
  expected: number;
}

export interface HostsUndoStatus {
  available: boolean;
  changedAt?: string;
  label?: string;
  reason?: string;
}

export interface TelemetryPolicy {
  revision: string;
  reviewedAt: string;
  reviewType: string;
  summary: string;
  sources: string[];
}

export interface HostsChangeReceipt {
  status: string;
  verified: boolean;
  changed?: boolean;
  dnsWarning?: string | null;
}

export interface ConnectionCheck {
  checkedAt: string;
  checks: Array<{ name: string; state: "ok" | "warning" | "unknown"; detail: string; affectedDomains?: string[] }>;
}

export function requireHostsChangeReceipt(result: { success: boolean; data?: HostsChangeReceipt; error?: string }): HostsChangeReceipt {
  if (!result.success || result.data?.status !== "success" || result.data.verified !== true) {
    throw new Error(result.error || "The blocking change could not be verified. Refresh protection to inspect its current state.");
  }
  return result.data;
}

export function hostsVerificationLabel(verification?: HostsVerification): string {
  switch (verification?.state) {
    case "verified": return "Applied and verified";
    case "partial": return "Partially applied";
    case "attention": return "Needs attention";
    case "off": return "Off";
    default: return "Verification unavailable";
  }
}
