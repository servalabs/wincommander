import type { Action, MatchKind, Rule, Severity } from "@/types/generated/fleet";

export type FleetRole = "viewer" | "operator" | "admin" | "super_admin";

export interface FleetSession {
  serverUrl: string;
  orgId: string;
  email: string;
  role: FleetRole;
  token: string;
  expiresAt: string;
}

export interface FleetOrgSettings {
  clipboard_guard_enabled: boolean;
  ink_receipt_enabled: boolean;
  ink_receipt_ticket_ttl_secs: number;
  ink_receipt_offline_max: number;
}

export type ClipboardAction = Action;
export type ClipboardMatcher = MatchKind;
export type ClipboardRule = Rule;
export type ClipboardSeverity = Severity;

export interface InkDestination {
  name: string;
  printerClass: "pdf" | "secure_physical";
}

export interface InkReceiptPolicy {
  managedDestinations: InkDestination[];
  ticketRequired: boolean;
  offlineBehavior: "ex_post_duplicate_detection";
  watermarkTemplate: string;
  failureStance: {
    pdf: "fail_closed" | "fail_soft";
    securePhysical: "fail_closed" | "fail_soft";
  };
}

export interface InkReceiptPolicyView extends InkReceiptPolicy {
  policy_version: number;
}
