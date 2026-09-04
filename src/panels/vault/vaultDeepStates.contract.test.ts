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
    expect(mountHandlerSource).toContain("getAvailableDriveLetters()");
    expect(mountHandlerSource).toContain("is already in use. Dismount it first or choose a free drive letter.");
    expect(vaultSource).toContain("vault_engine_unlock_failed");
    expect(vaultSource).toContain("entered its original password, PIM, and keyfile");
    expect(vaultSource).toContain("vault_broker_unavailable");
    expect(vaultSource).toContain("secure mount helper could not be reached");
    expect(vaultSource).toContain("vault_broker_rejected");
    expect(vaultSource).toContain("could not verify the secure mount helper");
    const cleanupMessage = vaultSource.slice(
      vaultSource.indexOf('if (normalized.includes("vault_cleanup_failed"))'),
      vaultSource.indexOf("return normalized.length"),
    );
    expect(cleanupMessage).toContain("mount state could not be confirmed after cleanup");
    expect(cleanupMessage).not.toContain("was not left mounted");
    expect(cleanupMessage).not.toContain("safely unmounted");
    expect(vaultSource).not.toContain("vault_broker_failed");
    const failureBranch = mountHandlerSource.slice(
      mountHandlerSource.indexOf("} catch"),
      mountHandlerSource.indexOf("} finally"),
    );
    expect(failureBranch).not.toContain("setMountDialogOpen(false)");
    expect(failureBranch).not.toContain("setMountPim");
  });

  test("mount credentials select the matching volume without exposing container internals", () => {
    expect(vaultSource).not.toContain("Hidden + decoy");
    expect(vaultSource).not.toContain("Open hidden volume");
    expect(vaultSource).not.toContain("Open visible decoy");
    expect(vaultSource).toContain('volumeKind: "standard"');
    expect(vaultSource).toContain('volumeRole: "standard"');
    expect(vaultSource).toContain("The password selects the matching standard, outer, or hidden volume automatically.");
    expect(mountHandlerSource).toContain("readOnly: true");
    expect(vaultSource).toContain("Read-only automatic mount");
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
    expect(sidebarSource).toContain('readOnly: true');
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
