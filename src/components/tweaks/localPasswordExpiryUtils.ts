export interface SkippedPasswordExpiryAccounts {
  builtIn: number;
  disabled: number;
  service: number;
  externalIdentity: number;
  nonUser: number;
}

export interface LocalPasswordExpiryStatus {
  isAdmin: boolean;
  totalEligible: number;
  passwordExpiresCount: number;
  passwordNeverExpiresCount: number;
  allEligibleNeverExpire: boolean;
  skipped: SkippedPasswordExpiryAccounts;
}

export function formatPasswordExpirySummary(status: Pick<LocalPasswordExpiryStatus, "passwordExpiresCount" | "totalEligible">): string {
  const noun = status.totalEligible === 1 ? "account" : "accounts";
  return `${status.passwordExpiresCount} of ${status.totalEligible} eligible local ${noun} still have password expiry enabled.`;
}

export function formatSkippedPasswordExpiryAccounts(skipped: SkippedPasswordExpiryAccounts): string | null {
  const parts = [
    [skipped.builtIn, "built-in"],
    [skipped.disabled, "disabled"],
    [skipped.service, "service"],
    [skipped.externalIdentity, "connected-work"],
    [skipped.nonUser, "non-user"],
  ].filter(([count]) => Number(count) > 0) as Array<[number, string]>;
  return parts.length ? `${parts.map(([count, label]) => `${count} ${label}`).join(", ")} account${parts.length === 1 && parts[0][0] === 1 ? "" : "s"} skipped.` : null;
}
