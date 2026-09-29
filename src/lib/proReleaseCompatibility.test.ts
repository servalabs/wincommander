import { describe, expect, test } from "bun:test";
import { parseProReleaseVersion, proReleaseCompatibilityError } from "./proReleaseCompatibility";
import { isProVersionCompatible } from "../hooks/useProInstall";

describe("Pro download compatibility", () => {
  test("rejects published 3.6.3 for 3.6.4 and explains why repeating repair cannot help", () => {
    expect(proReleaseCompatibilityError("3.6.3", "3.6.4")).toContain("reinstalling the older Pro will not fix Vault mounting");
    expect(isProVersionCompatible("3.6.3", "3.6.4")).toBe(false);
  });

  test("accepts matching three-part and Windows four-part release versions", () => {
    expect(proReleaseCompatibilityError("3.6.4.0", "3.6.4")).toBeNull();
    expect(proReleaseCompatibilityError("v3.6.4", "3.6.4.0")).toBeNull();
    expect(proReleaseCompatibilityError("3.6.4", "3.6.5")).toBeNull();
    expect(isProVersionCompatible("3.6.4", "3.6.4")).toBe(true);
  });

  test("refuses newer Pro until Free is updated", () => {
    expect(proReleaseCompatibilityError("3.6.5", "3.6.4")).toContain("Update WinCommander first");
  });

  test("fails closed for missing, partial or malformed version data", () => {
    for (const invalid of [null, undefined, "", "unknown", "3.6", "3.6.4-beta", "prefix3.6.4", "3.6.4.0.1", "3.6.4+bad"]) {
      expect(parseProReleaseVersion(invalid)).toBeNull();
      expect(isProVersionCompatible(invalid, "3.6.4")).toBe(false);
      expect(isProVersionCompatible("3.6.4", invalid)).toBe(false);
    }
  });
});
