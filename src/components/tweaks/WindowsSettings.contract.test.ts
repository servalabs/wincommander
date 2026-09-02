import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const read = (path: string) => Bun.file(path).text();

describe("Windows Settings layout contracts", () => {
  test("advanced AI cleanup keeps every operation as a persisted universal toggle", async () => {
    const component = await read("src/components/tweaks/WindowsAiAdvancedActions.tsx");

    expect(component).toContain("aiComponentCleanup");
    expect(component).toContain("<UniversalToggle");
    expect(component).toContain('size="compact"');
    expect(component).toContain("onChange");
    expect(component).not.toContain("<Switch");
    expect(component).not.toContain("Manual cleanup checks");
    expect(component).not.toContain("Run once");
  });

  test("power and graphics share one responsive decision card", async () => {
    const panel = await read("src/panels/tweaks/index.tsx");
    const card = await read("src/components/tweaks/PowerGraphicsCard.tsx");

    expect(panel).toContain("<PowerGraphicsCard");
    expect(card).toContain('title="Power & Graphics"');
    expect(card).toContain("Power management");
    expect(card).toContain("Graphics — detected vendor");
  });

  test("Windows Settings keeps acquisition monitoring but not the Maintenance-only driver scan", async () => {
    const panel = await read("src/panels/tweaks/index.tsx");
    const extras = await read("src/components/tweaks/ExploitProtectionExtras.tsx");

    expect(panel).toContain("headerRight={<AcquisitionMonitorSwitch />}");
    expect(panel).not.toContain("<ExploitProtectionExtras");
    expect(extras).not.toContain("Vulnerable driver scan");
    expect(extras).not.toContain("Microsoft’s vulnerable-driver blocklist");
  });

  test("context actions and every security or performance tile have an icon", async () => {
    const contextMenu = await read("src/components/tweaks/ContextMenuIntegrationCard.tsx");
    const registry = await read("src/registry/tweaks.toggles.ts");

    expect(contextMenu).toContain('label="Secure Delete"');
    expect(contextMenu).toContain('icon="trash"');
    expect(contextMenu).toContain('label="Scrub metadata"');
    expect(contextMenu).toContain('icon="eraser"');
    expect(contextMenu).toContain('label="Safe Copy / Safe Paste"');
    expect(contextMenu).toContain('icon="clipboard"');

    const cards = registry.split(/^  \{\n/m);
    for (const section of ["security", "performance"]) {
      const sectionCards = cards.filter((card) => card.includes(`section: "${section}"`));
      expect(sectionCards.length).toBeGreaterThan(0);
      for (const card of sectionCards) {
        expect(card).toMatch(/\n\s*icon:\s*"[^"]+",/);
      }
    }
  });
});
