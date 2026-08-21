import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const vaultSource = readFileSync("src/panels/vault/index.tsx", "utf8");
const ramDiskSource = readFileSync("src/panels/vault/CreateRamDiskDialog.tsx", "utf8");
const propertiesSource = readFileSync("src/panels/vault/VolumePropertiesDialog.tsx", "utf8");
const backendSource = readFileSync("src/hooks/useBackend.ts", "utf8");
const sidebarSource = readFileSync("src/components/RightSidebar.tsx", "utf8");
const appContextSource = readFileSync("src/context/AppContext.tsx", "utf8");

describe("secure storage deep-state contracts", () => {
  test("distinguishes initial loading from a confirmed empty volume list", () => {
    expect(vaultSource).toContain('initialLoading={loading.vault && encryptionStatus === null}');
    expect(vaultSource).toContain('aria-busy="true"');
    expect(vaultSource).toContain("Checking encrypted volumes…");
  });

  test("partition discovery exposes progress instead of flashing an empty result", () => {
    expect(vaultSource).toContain("setMountDetailsLoading(true)");
    expect(vaultSource).toContain("Discovering partitions…");
    expect(vaultSource).toContain("setMountDetailsLoading(false)");
  });

  test("native failures remain visible and announced", () => {
    expect(ramDiskSource).toContain("catch (error)");
    expect(propertiesSource).toContain('className="props-error" role="alert"');
  });

  test("emergency backup registration is paid, no-path, and one-mount only", () => {
    expect(vaultSource).toContain('{paid && volumes.length === 1 && (');
    expect(vaultSource).toContain('getEncryptedBackupTargetStatus');
    expect(vaultSource).toContain('provisionEncryptedBackupTarget()');
    expect(vaultSource).toContain('clearEncryptedBackupTarget()');
    expect(vaultSource).toContain('volumes.length !== 1');
    expect(vaultSource).toContain('This screen accepts no path');
    expect(vaultSource).not.toContain('provisionEncryptedBackupTarget(' + 'mountPath');
    expect(backendSource).toContain(
      'execute<{ ok: boolean; bound: boolean; version: number }>("Provision-EncryptedBackupTarget")',
    );
    expect(backendSource).toContain(
      'execute<{ ok: boolean; cleared: boolean }>("Clear-EncryptedBackupTarget")',
    );
  });

  test("every interactive mount is private and verifies the Explorer-facing drive", () => {
    expect(vaultSource).toContain('scope: "per-user"');
    expect(vaultSource).toContain("hardenAcl: true");
    expect(vaultSource).toContain("await verifyVaultDrive(result.data.drive)");
    expect(vaultSource).toContain("setMountedVolume(result.data)");
    expect(vaultSource).toContain('icon="warning-sign"');
    expect(sidebarSource).toContain('scope: "per-user"');
    expect(sidebarSource).toContain("hardenAcl: true");
    expect(sidebarSource).toContain("await verifyVaultDrive(r.data.drive)");
    expect(backendSource).toContain('invoke<{ drive: string; accessible: boolean }>("verify_vault_drive"');
    expect(appContextSource).toContain("setEncryptionStatus(null);");
  });
});
