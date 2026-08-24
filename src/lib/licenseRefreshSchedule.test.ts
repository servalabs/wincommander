import { describe, expect, test } from "bun:test";
import { LICENSE_REFRESH_BASE_MS, nextLicenseRefreshDelay } from "./licenseRefreshSchedule";

describe("license refresh schedule", () => {
  test("keeps normal checks near twice per day with bounded jitter", () => {
    expect(nextLicenseRefreshDelay(0, () => 0)).toBe(LICENSE_REFRESH_BASE_MS - 60 * 60 * 1_000);
    expect(nextLicenseRefreshDelay(0, () => 1)).toBe(LICENSE_REFRESH_BASE_MS + 60 * 60 * 1_000);
  });

  test("backs off retries instead of repeatedly calling the licence service", () => {
    expect(nextLicenseRefreshDelay(1)).toBe(15 * 60 * 1_000);
    expect(nextLicenseRefreshDelay(2)).toBe(60 * 60 * 1_000);
    expect(nextLicenseRefreshDelay(3)).toBe(4 * 60 * 60 * 1_000);
  });
});
