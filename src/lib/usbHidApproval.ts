export const DEFAULT_USB_HID_APPROVAL_TTL_SECS = 60;
export const USB_HID_APPROVAL_CHALLENGE_LENGTH = 6;

export type UsbHidApprovalStatus =
  | "pending"
  | "approvedOnce"
  | "trustedAlways"
  | "blocked"
  | "expired"
  | "containmentFailed";

export interface PendingUsbHidApproval {
  deviceKey: string;
  friendlyName: string;
  manufacturer: string | null;
  vid: string | null;
  pid: string | null;
  serialStable: boolean;
  topologyVerified: boolean;
  requestedAt: string;
  expiresAt: string;
  expiresAtEpoch: number;
  status: UsbHidApprovalStatus;
  enforcement: {
    attempted: boolean;
    succeeded: boolean;
    error?: string;
  };
  reactive: true;
  firstKeystrokePrevention: false;
  prebootPrevention: false;
  requiresFamilyTrustConfirmation?: boolean;
  trustAlwaysWarning?: string;
  topologyWarning?: string;
  availableActions?: Array<"allowOnce" | "trustAlways" | "block">;
  challengeLockedUntilEpoch?: number;
  lastDecisionError?: string;
}

export interface UsbHidApprovalGateStatus {
  running: boolean;
  approvalTtlSecs: number;
  pendingCount: number;
  containmentFailedCount: number;
  reactive: true;
  firstKeystrokePrevention: false;
  prebootPrevention: false;
}

export interface UsbHidApprovalList {
  items: PendingUsbHidApproval[];
  nowEpoch: number;
}

export interface UsbHidVisualChallenge {
  challengeId: string;
  deviceKey: string;
  action: "allowOnce" | "trustAlways";
  displaySequence: string;
  keypadLayout: string[];
  digitsAccepted: number;
  digitsRemaining: number;
  step: number;
  totalSteps: number;
  expiresAtEpoch: number;
  expiresAt: string;
  reactive: true;
  firstKeystrokePrevention: false;
  prebootPrevention: false;
  userPresenceOnly: true;
  sourceAttribution: "unavailable";
  warning: string;
}

export interface UsbHidVisualChallengeFollowUp {
  status: "challengeRequired";
  challenge: UsbHidVisualChallenge;
}

export interface UsbHidVisualChallengeProgress {
  status: "progress";
  challenge: UsbHidVisualChallenge;
}

export interface UsbHidVisualChallengeLocked {
  status: "challengeLocked";
  retryAfterEpoch: number;
}

export type UsbHidVisualChallengeDigitResult =
  | PendingUsbHidApproval
  | UsbHidVisualChallengeFollowUp
  | UsbHidVisualChallengeProgress
  | UsbHidVisualChallengeLocked;

export function pendingApprovals(items: PendingUsbHidApproval[]): PendingUsbHidApproval[] {
  return items.filter((item) => item.status === "pending" || item.status === "containmentFailed");
}

export function canApproveWithPointer(pointerType: string, button: number): boolean {
  return pointerType === "mouse" && button === 0;
}

export function isValidVisualChallenge(challenge: UsbHidVisualChallenge): boolean {
  return new RegExp(`^\\d{${USB_HID_APPROVAL_CHALLENGE_LENGTH}}$`).test(challenge.displaySequence)
    && challenge.keypadLayout.length === 10
    && new Set(challenge.keypadLayout).size === 10
    && challenge.keypadLayout.every((digit) => /^\d$/.test(digit))
    && challenge.digitsAccepted >= 0
    && challenge.digitsAccepted <= USB_HID_APPROVAL_CHALLENGE_LENGTH
    && challenge.digitsRemaining === USB_HID_APPROVAL_CHALLENGE_LENGTH - challenge.digitsAccepted;
}

export function isVisualChallengeFollowUp(
  result: UsbHidVisualChallengeDigitResult,
): result is UsbHidVisualChallengeFollowUp {
  return result.status === "challengeRequired";
}

export function isVisualChallengeProgress(
  result: UsbHidVisualChallengeDigitResult,
): result is UsbHidVisualChallengeProgress {
  return result.status === "progress";
}

export function isVisualChallengeLocked(
  result: UsbHidVisualChallengeDigitResult,
): result is UsbHidVisualChallengeLocked {
  return result.status === "challengeLocked";
}

export function deviceLabel(item: PendingUsbHidApproval): string {
  return item.friendlyName.trim() || "Unnamed input device";
}

export function deviceFingerprint(item: PendingUsbHidApproval): string | null {
  if (!item.vid || !item.pid) return null;
  return `${item.vid}:${item.pid}`.toUpperCase();
}
