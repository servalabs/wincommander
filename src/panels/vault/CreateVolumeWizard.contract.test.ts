import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("Create encrypted volume wizard layout", () => {
  test("requires a complex ten-character password before creation", async () => {
    const wizard = await Bun.file("src/panels/vault/CreateVolumeWizard.tsx").text();

    expect(wizard).toContain("MIN_CREATION_PASSWORD_LENGTH = 10");
    expect(wizard).toContain("/[A-Za-z]/.test(password)");
    expect(wizard).toContain("/\\d/.test(password)");
    expect(wizard).toContain("/[^A-Za-z0-9]/.test(password)");
    expect(wizard).toContain("letter, a number, and a special character");
  });

  test("does not expose new-volume passwords to browser or password-manager autofill", async () => {
    const [wizard, inputGroup] = await Promise.all([
      Bun.file("src/panels/vault/CreateVolumeWizard.tsx").text(),
      Bun.file("src/components/ui/bp.tsx").text(),
    ]);

    expect(wizard).toContain('<form className="wizard-step-content" autoComplete="off"');
    expect(wizard).toContain('name="wincommander-new-volume-password"');
    expect(wizard).not.toContain('autoComplete="new-password"');
    expect(wizard.match(/data-1p-ignore="true"/g)?.length).toBe(4);
    expect(wizard.match(/data-bwignore="true"/g)?.length).toBe(4);
    expect(wizard.match(/data-lpignore="true"/g)?.length).toBe(4);
    expect(inputGroup).toContain('data-1p-ignore={dataOnePasswordIgnore}');
    expect(inputGroup).toContain('data-bwignore={dataBitwardenIgnore}');
    expect(inputGroup).toContain('data-lpignore={dataLastPassIgnore}');
  });

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

  test("requires a fresh acknowledgment before erasing a selected partition", async () => {
    const wizard = await Bun.file("src/panels/vault/CreateVolumeWizard.tsx").text();

    expect(wizard).toContain("deviceEraseAcknowledged");
    expect(wizard).toContain("Boolean(selectedPartition?.safeForCreation) && deviceEraseAcknowledged");
    expect(wizard).toContain("setDeviceEraseAcknowledged(false)");
    expect(wizard).toContain("Only partitions WinCommander identifies as safe are listed");
    expect(wizard).toContain("permanently erase Disk {selectedPartition.diskNumber}");
  });
});
