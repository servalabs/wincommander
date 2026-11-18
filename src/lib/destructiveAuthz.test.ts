import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";

describe("destructive confirmation request", () => {
  it("requires capabilities at both user-facing generic erase call sites", () => {
    const source = readFileSync("src/hooks/useBackend.ts", "utf8");
    expect(source).toMatch(
      /invokeUnallocatedSpaceErase:[\s\S]*requestDestructiveCapability\([\s\S]*command: "free_space_erase"[\s\S]*CapabilityToken: capabilityToken/,
    );
    expect(source).toMatch(
      /invoke7Erase:[\s\S]*requestDestructiveCapability\(\{ command: "secure_erase", path \}\)[\s\S]*invoke\("secure_shred", \{ path, capabilityToken \}\)/,
    );
  });
});
