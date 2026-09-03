import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { requestDestructiveCapability } from "../hooks/destructiveAuthz";

describe("destructive confirmation request", () => {
  it("has a typed Rust-canonicalized request API", () => {
    expect(typeof requestDestructiveCapability).toBe("function");
  });

  it("keeps secure erase and free-space erase in the typed request contract", () => {
    const source = readFileSync("src/hooks/destructiveAuthz.ts", "utf8");
    expect(source).toContain('{ command: "secure_erase"; path: string }');
    expect(source).toContain(
      '{ command: "free_space_erase"; driveLetter: string; mediaType: string }',
    );
    expect(source).toContain('command: "selective_crypto_erase"');
  });

  it("requires capabilities at both user-facing generic erase call sites", () => {
    const source = readFileSync("src/hooks/useBackend.ts", "utf8");
    expect(source).toMatch(
      /invokeUnallocatedSpaceErase:[\s\S]*requestDestructiveCapability\([\s\S]*command: "free_space_erase"[\s\S]*CapabilityToken: capabilityToken/,
    );
    expect(source).toMatch(
      /invoke7Erase:[\s\S]*requestDestructiveCapability\(\{ command: "secure_erase", path \}\)[\s\S]*CapabilityToken: capabilityToken/,
    );
  });

  it("does not expose renderer cancellation for Rust-owned lockdown triggers", () => {
    const handlerSource = readFileSync(
      "src-tauri/commander-free/src/lib.rs",
      "utf8",
    );
    const sidebarSource = readFileSync("src/components/RightSidebar.tsx", "utf8");
    expect(handlerSource).not.toContain("authz::abort_trusted_lockdown");
    expect(sidebarSource).not.toContain("invoke('abort_trusted_lockdown')");
  });
});
