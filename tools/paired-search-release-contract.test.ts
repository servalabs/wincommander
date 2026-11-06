import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { pairedSearchTemplate, pairedServiceHash } from "./paired-search-release-contract";

describe("paired search packaging contract", () => {
  test("enables both maintenance defines only with the expected hook", () => {
    const template = 'Unicode true\n!include "{{installer_hooks}}"\n';
    const result = pairedSearchTemplate(template);
    expect(result).toContain("!define WC_EXPERIMENTAL_USER_STAGE_ONLY");
    expect(result).toContain("!define WC_EXPERIMENTAL_SEARCH_FENCE");
    expect(() => pairedSearchTemplate("Unicode true")).toThrow();
    expect(() => pairedSearchTemplate(result)).toThrow();
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
