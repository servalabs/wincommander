import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const vaultSource = readFileSync("src/panels/vault/index.tsx", "utf8");
const ramDiskSource = readFileSync("src/panels/vault/CreateRamDiskDialog.tsx", "utf8");
const propertiesSource = readFileSync("src/panels/vault/VolumePropertiesDialog.tsx", "utf8");
const backendSource = readFileSync("src/hooks/useBackend.ts", "utf8");

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
});
