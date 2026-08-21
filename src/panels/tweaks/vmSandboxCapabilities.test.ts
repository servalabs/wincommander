import { describe, expect, test } from "bun:test";
import { supportsWindowsSandbox, type VmCapabilities } from "./vmSandboxCapabilities";

const clientCapabilities: VmCapabilities = {
  platform: "windows-client",
  hyperv: true,
  sandbox: false,
  sandboxSupported: true,
  hypervFeature: "Enabled",
};

describe("Windows Sandbox capability visibility", () => {
  test("hides Windows Sandbox when the server capability probe rejects it", () => {
    expect(supportsWindowsSandbox({ ...clientCapabilities, platform: "windows-server", sandboxSupported: false })).toBe(false);
    expect(supportsWindowsSandbox({ ...clientCapabilities, platform: "windows-server" })).toBe(false);
  });

  test("does not expose Windows Sandbox until the backend confirms support", () => {
    expect(supportsWindowsSandbox(null)).toBe(false);
    expect(supportsWindowsSandbox({ ...clientCapabilities, platform: "unknown", sandboxSupported: false })).toBe(false);
  });

  test("shows Windows Sandbox on a supported client SKU", () => {
    expect(supportsWindowsSandbox(clientCapabilities)).toBe(true);
  });
});
