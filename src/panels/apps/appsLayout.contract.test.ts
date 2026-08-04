import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const read = (path: string) => Bun.file(path).text();

describe("Packages and Apps layout", () => {
  test("keeps package updates inside Install software's Updates view", async () => {
    const panel = await read("src/panels/apps/index.tsx");
    const installer = await read("src/panels/apps/components/AppInstallerPanel.tsx");

    expect(panel).not.toContain('value="updates-tools"');
    expect(panel).toContain("updatesTools={<div");
    expect(installer).toContain('<TabsContent value="updates">');
    expect(installer).toContain("{updatesTools &&");
  });

  test("keeps optional managers out of engine readiness and removes obsolete package surfaces", async () => {
    const engines = await read("src/panels/apps/components/EnginesSection.tsx");
    const panel = await read("src/panels/apps/index.tsx");

    expect(engines).toContain('"chocolatey"');
    expect(engines).toContain('"scoop"');
    expect(engines).not.toContain('"encryptionEngine"');
    expect(panel).not.toContain("ClassicWindowsApps");
    expect(panel).not.toContain("classic-photo-viewer");
  });

  test("moves successful installs to Installed without waiting for the inventory refresh", async () => {
    const installer = await read("src/panels/apps/components/AppInstallerPanel.tsx");
    const engines = await read("src/panels/apps/components/EnginesSection.tsx");

    expect(installer).toContain("setInstalledApps(prev => new Set(prev).add(id));");
    expect(installer).toContain('setInstallerView("installed")');
    expect(engines).not.toContain("eng-installed");
  });
});
