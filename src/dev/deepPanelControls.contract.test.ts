import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): { text(): Promise<string> };
};

describe("deep panel control accessibility", () => {
  test("uses one semantic switch per toggle tile", async () => {
    const [source, bp] = await Promise.all([
      Bun.file("src/components/shared/ToggleTile.tsx").text(),
      Bun.file("src/components/ui/bp.tsx").text(),
    ]);

    expect(source).not.toContain('role="switch"');
    expect(source).toContain("<WCSwitch");
    expect(source).toContain("label={label}");
    expect(bp).toContain('"aria-label": ariaLabel');
    expect(bp).toContain("aria-label={ariaLabel}");
  });

  test("labels controls that rendered anonymously in the deep audit", async () => {
    const [vpn, fleet, dev, flows, apps, wizard, vault, volumeActions, ramDisks, createRamDisk, stego, rdp] = await Promise.all([
      Bun.file("src/panels/mesh/VpnKillSwitchSection.tsx").text(),
      Bun.file("src/panels/fleet/FleetConnectView.tsx").text(),
      Bun.file("src/panels/dev/index.tsx").text(),
      Bun.file("src/panels/flows/RuleEditor.tsx").text(),
      Bun.file("src/panels/server-apps/ManageAppsDialog.tsx").text(),
      Bun.file("src/panels/vault/CreateVolumeWizard.tsx").text(),
      Bun.file("src/panels/vault/index.tsx").text(),
      Bun.file("src/panels/vault/VolumeActionsMenu.tsx").text(),
      Bun.file("src/panels/vault/RamDisksSection.tsx").text(),
      Bun.file("src/panels/vault/CreateRamDiskDialog.tsx").text(),
      Bun.file("src/panels/vault/StegoBackupSection.tsx").text(),
      Bun.file("src/components/RdpQuickAction.tsx").text(),
    ]);

    expect(vpn).toContain('aria-label="Block internet if VPN drops"');
    expect(fleet).toContain('aria-label="Enable command dispatch"');
    expect(dev).toContain('aria-label="Simulated event type"');
    expect(flows).toContain('aria-label="Start hour"');
    expect(flows).toContain('aria-label="End hour"');
    expect(apps).toContain("aria-label={`Move ${row.name} up`}");
    expect(apps).toContain("labelFor={`manage-app-icon-${row._stableKey}`}");
    expect(wizard).toContain('title="Create Encrypted Volume"');
    expect(vault).toContain('" type-badge--hidden"');
    expect(vault).not.toContain('" hidden"');
    expect(volumeActions).toContain('const driveLabel = letter.endsWith(":") ? letter : `${letter}:`;');
    expect(volumeActions).toContain("aria-label={`Open ${driveLabel} in Explorer`}");
    expect(volumeActions).toContain("aria-label={`Dismount ${driveLabel}`}");
    expect(ramDisks).toContain('aria-label="Auto-create RAM disk on startup"');
    expect(ramDisks).toContain("aria-label={`Open ${d.letter} in Explorer`}");
    expect(ramDisks).toContain('aria-label="Startup RAM disk size"');
    expect(createRamDisk).toContain('aria-label="RAM disk size"');
    expect(stego).toContain('labelFor="stego-hidden-volume-size"');
    expect(stego).toContain('labelFor="stego-password"');
    expect(stego).toContain('labelFor="stego-password-confirm"');
    expect(rdp).toContain("aria-label={`Edit ${n.label}`}");
    expect(rdp).toContain("aria-label={`Delete ${n.label}`}");
    expect(rdp).toContain('aria-label="Manage remote endpoints"');
    expect(rdp).toContain("aria-expanded={isListOpen}");
    expect(rdp).not.toContain('role="button"');
  });
});
