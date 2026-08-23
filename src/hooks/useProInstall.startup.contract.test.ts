import { describe, expect, test } from "bun:test";

declare const Bun: { file(path: URL): { text(): Promise<string> } };

describe("Pro startup probe policy", () => {
  test("gates automatic Pro work and routes it through the shared coordinator", async () => {
    const source = await Bun.file(new URL("./useProInstall.ts", import.meta.url)).text();

    expect(source).toContain("if (policy.manifest");
    expect(source).toContain("if (policy.status");
    expect(source).toContain("if (policy.defender");
    expect(source).toContain('id: "pro-manifest"');
    expect(source).toContain('id: "pro-install-status"');
    expect(source).toContain('id: "defender-status"');
  });

  test("does not arm the passive Pro prompt for a Free launch", async () => {
    const source = await Bun.file(new URL("../App.tsx", import.meta.url)).text();

    expect(source).toContain("hasStartupProEntitlement");
    expect(source).toContain("status: shouldProbeProPrompt");
    expect(source).toContain("manifest: shouldProbeProPrompt");
    expect(source).toContain("defender: false");
  });
});
