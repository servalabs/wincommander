import { describe, expect, test } from "bun:test";
import { formatPasswordExpirySummary, formatSkippedPasswordExpiryAccounts } from "./localPasswordExpiryUtils";

describe("local password expiry summary", () => {
  test("reports the accounts that still expire when the state is mixed", () => {
    expect(formatPasswordExpirySummary({ totalEligible: 8, passwordExpiresCount: 5 }))
      .toBe("5 of 8 eligible local accounts still have password expiry enabled.");
  });

  test("reports a fully enabled never-expire state without changing the count", () => {
    expect(formatPasswordExpirySummary({ totalEligible: 8, passwordExpiresCount: 0 }))
      .toBe("0 of 8 eligible local accounts still have password expiry enabled.");
  });

  test("makes skipped account categories visible instead of silently bulk-changing them", () => {
    expect(formatSkippedPasswordExpiryAccounts({
      builtIn: 2, disabled: 1, service: 1, externalIdentity: 0, nonUser: 0,
    })).toBe("2 built-in, 1 disabled, 1 service accounts skipped.");
  });
});
