import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Create encrypted volume wizard layout", () => {
  test("reserves a persistent footer for Back and Next", async () => {
    const [wizard, styles, dialog] = await Promise.all([
      Bun.file("src/panels/vault/CreateVolumeWizard.tsx").text(),
      Bun.file("src/panels/vault/CreateVolumeWizard.css").text(),
      Bun.file("src/components/ui/bp.tsx").text(),
    ]);

    expect(wizard).toContain("hideHeader");
    expect(wizard).toContain('text="BACK"');
    expect(wizard).toContain('text="NEXT"');
    expect(styles).toContain(".wizard-body");
    expect(styles).toContain("overflow-y: auto");
    expect(styles).toContain(".wizard-footer");
    expect(styles).toContain("flex: 0 0 auto");
    expect(dialog).toContain("hideHeader = false");
    expect(dialog).toContain('{title ?? "Dialog"}');
  });
});
