import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { pairedServiceHash, pairedSearchTemplate } from "./paired-search-release-contract";

describe("paired search packaging contract", () => {
  test("binds paired writer checks to the installer template", () => {
    const template = '!include "{{installer_hooks}}"';
    expect(pairedSearchTemplate(template)).toContain("!define WC_PAIRED_SEARCH_RELEASE");
    expect(() => pairedSearchTemplate("missing hook")).toThrow();
    expect(() => pairedSearchTemplate(pairedSearchTemplate(template))).toThrow();
  });
  test("requires the actual nonzero final service hash", () => {
    const bytes = new TextEncoder().encode("fixture service");
    const hash = createHash("sha256").update(bytes).digest("hex");
    expect(pairedServiceHash(bytes, hash.toUpperCase())).toBe(hash);
    expect(() => pairedServiceHash(bytes, "0".repeat(64))).toThrow();
    expect(() => pairedServiceHash(bytes, "a".repeat(64))).toThrow();
    expect(() => pairedServiceHash(bytes, "broken")).toThrow();
  });
});
