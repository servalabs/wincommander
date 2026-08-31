import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const vaultSource = readFileSync("src/panels/vault/index.tsx", "utf8");
const ramDiskSource = readFileSync("src/panels/vault/CreateRamDiskDialog.tsx", "utf8");
const propertiesSource = readFileSync("src/panels/vault/VolumePropertiesDialog.tsx", "utf8");
const backendSource = readFileSync("src/hooks/useBackend.ts", "utf8");
const sidebarSource = readFileSync("src/components/RightSidebar.tsx", "utf8");
const appContextSource = readFileSync("src/context/AppContext.tsx", "utf8");
const mountHandlerSource = vaultSource.slice(
  vaultSource.indexOf("const handleMountVolume"),
  vaultSource.indexOf("const handleOpenMountedVolume"),
);
const systemEncryptionSource = readFileSync("src/panels/vault/SystemEncryptionSection.tsx", "utf8");
const createVolumeWizardSource = readFileSync("src/panels/vault/CreateVolumeWizard.tsx", "utf8");

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
    expect(vaultSource).toContain('className="mount-error" role="alert"');
    expect(vaultSource).toContain('className="mount-progress" role="status"');
    expect(vaultSource).toContain("Unlocking with your PIM can take several minutes. Your password was cleared for safety.");
    expect(vaultSource).toContain("const boundedMountError");
    expect(mountHandlerSource).toContain("setMountFailure(message);");
    expect(mountHandlerSource).toContain("const mountRequest = mountVolume({");
    expect(mountHandlerSource).toContain("setMountPassword(\"\");");
    expect(mountHandlerSource).toContain("setHiddenPassword(\"\");");
    expect(mountHandlerSource).toContain("getAvailableDriveLetters()");
    expect(mountHandlerSource).toContain("is already in use. Dismount it first or choose a free drive letter.");
    expect(vaultSource).toContain("vault_engine_unlock_failed");
    expect(vaultSource).toContain("entered its original password, PIM, and keyfile");
    const failureBranch = mountHandlerSource.slice(
      mountHandlerSource.indexOf("} catch"),
      mountHandlerSource.indexOf("} finally"),
    );
    expect(failureBranch).not.toContain("setMountDialogOpen(false)");
    expect(failureBranch).not.toContain("setMountPim");
    expect(failureBranch).not.toContain("setHiddenPim");
    expect(failureBranch).not.toContain("setProtectHidden");
  });

  test("dual-volume mount makes its target explicit and protects writable decoys", () => {
    expect(vaultSource).toContain('selectVolumeKind("dual")');
    expect(vaultSource).toContain('selectVolumeRole("outer")');
    expect(vaultSource).toContain('selectVolumeRole("hidden")');
    expect(mountHandlerSource).toContain("volumeKind,");
    expect(mountHandlerSource).toContain("volumeRole,");
    expect(vaultSource).toContain("Open hidden volume");
    expect(vaultSource).toContain("Open visible decoy");
    expect(vaultSource).toContain('label={volumeRole === "hidden" ? "Hidden volume password" : "Password"}');
    expect(vaultSource).toContain("const requiresHiddenProtection = isOuterDualVolume && !mountReadOnly;");
    expect(vaultSource).toContain("protectHidden: requiresHiddenProtection && protectHidden");
    expect(backendSource).toContain("VolumeKind: params.volumeKind");
    expect(backendSource).toContain("VolumeRole: params.volumeRole");
  });

  test("separates driver-only mounts from drives this Windows session can open", () => {
    expect(vaultSource).toContain('volume.accessible !== false');
    expect(vaultSource).toContain("needs attention");
    expect(vaultSource).toContain("Not available in this Windows sign-in");
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
    expect(vaultSource).not.toContain("Only this Windows account");
    expect(vaultSource).not.toContain("The drive will not appear in other users’ File Explorer sessions.");
    expect(backendSource).toContain("PresentedLetter: letter");
  });

  test("does not spawn an unsupported system-encryption probe when Secure Storage opens", () => {
    expect(vaultSource).toContain("<SystemEncryptionSection />");
    expect(systemEncryptionSource).toContain("Not verified");
    expect(systemEncryptionSource).not.toContain("useBackend");
    expect(systemEncryptionSource).not.toContain("Get-SystemEncryptionStatus");
    expect(systemEncryptionSource).not.toContain("useEffect");
  });

  test("allows a reviewed safe partition to continue without a typed erase phrase", () => {
    expect(createVolumeWizardSource).toContain("Boolean(selectedPartition?.safeForCreation)");
    expect(createVolumeWizardSource).not.toContain("deviceConfirmation");
    expect(createVolumeWizardSource).not.toContain("Destructive confirmation");
    expect(backendSource).not.toContain("DeviceConfirmation: params.Device.confirmation");
  });
});
