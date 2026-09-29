import { describe, expect, test } from "bun:test";
import {
  LICENSE_REFRESH_BASE_MS,
  LICENSE_REFRESH_INITIAL_DELAY_MS,
  nextLicenseRefreshDueDelay,
  nextLicenseRefreshRetryDelay,
} from "./licenseRefreshSchedule";

describe("license refresh schedule", () => {
  test("schedules from the persisted verification time without refreshing early", () => {
    const now = 2_000_000_000_000;
    const verifiedNow = now / 1_000;
    const verifiedElevenHoursFiftyNineMinutesAgo = (now - LICENSE_REFRESH_BASE_MS + 60_000) / 1_000;

    expect(nextLicenseRefreshDueDelay(verifiedNow, now, () => 0)).toBe(LICENSE_REFRESH_BASE_MS);
    expect(nextLicenseRefreshDueDelay(verifiedNow, now, () => 1)).toBe(
      LICENSE_REFRESH_BASE_MS + 5 * 60 * 1_000,
    );
    expect(nextLicenseRefreshDueDelay(verifiedElevenHoursFiftyNineMinutesAgo, now, () => 0)).toBe(60_000);
  });

  test("schedules an immediate native check when verification is absent or overdue", () => {
    const now = 2_000_000_000_000;

    expect(nextLicenseRefreshDueDelay(undefined, now)).toBe(LICENSE_REFRESH_INITIAL_DELAY_MS);
    expect(nextLicenseRefreshDueDelay((now - LICENSE_REFRESH_BASE_MS) / 1_000, now)).toBe(
      LICENSE_REFRESH_INITIAL_DELAY_MS,
    );
    expect(nextLicenseRefreshDueDelay(now / 1_000 + 1, now)).toBe(LICENSE_REFRESH_INITIAL_DELAY_MS);
  });

  test("backs off retries instead of repeatedly calling the licence service", () => {
    expect(nextLicenseRefreshRetryDelay(1)).toBe(15 * 60 * 1_000);
    expect(nextLicenseRefreshRetryDelay(2)).toBe(60 * 60 * 1_000);
    expect(nextLicenseRefreshRetryDelay(3)).toBe(4 * 60 * 60 * 1_000);
  });
});
