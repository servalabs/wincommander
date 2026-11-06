import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { pairedServiceHash } from "./paired-search-release-contract";

describe("paired search packaging contract", () => {
  test("requires the actual nonzero final service hash", () => {
    const bytes = new TextEncoder().encode("fixture service");
    const hash = createHash("sha256").update(bytes).digest("hex");
    expect(pairedServiceHash(bytes, hash.toUpperCase())).toBe(hash);
    expect(() => pairedServiceHash(bytes, "0".repeat(64))).toThrow();
    expect(() => pairedServiceHash(bytes, "a".repeat(64))).toThrow();
    expect(() => pairedServiceHash(bytes, "broken")).toThrow();
  });
});
