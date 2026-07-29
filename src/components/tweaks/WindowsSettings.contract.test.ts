import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

const read = (path: string) => Bun.file(path).text();

describe("Windows Settings layout contracts", () => {
  test("advanced AI cleanup keeps every operation as a persisted toggle", async () => {
    const component = await read("src/components/tweaks/WindowsAiAdvancedActions.tsx");

    expect(component).toContain("aiComponentCleanup");
    expect(component).toContain("onCheckedChange");
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
});
