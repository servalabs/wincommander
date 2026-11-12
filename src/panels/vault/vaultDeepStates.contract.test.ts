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
    expect(vaultSource).toContain("Checking mounted-volume access…");
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
    expect(vaultSource).toContain("vault_container_already_mounted");
    expect(vaultSource).toContain("A container with this filename is already mounted.");
    expect(vaultSource).toContain('return "VLT.CONTAINER.ALREADY_MOUNTED";');
    expect(vaultSource).toContain("installed WinCommander service still requires an NTFS permission check");
    expect(vaultSource).toContain("Repair or update the WinCommander service");
    expect(mountHandlerSource).toContain("setMountFailure(message);");
    expect(mountHandlerSource).toContain("mountPasswordSelectedVolume(async (params)");
    expect(mountHandlerSource).toContain("mountPersonalVolume({");
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
      mountHandlerSource.lastIndexOf("} catch"),
      mountHandlerSource.indexOf("} finally"),
    );
    expect(failureBranch).not.toContain("setMountDialogOpen(false)");
    expect(failureBranch).not.toContain("setMountPim");
  });

  test("mount credentials select the matching volume without exposing container internals", () => {
    expect(vaultSource).not.toContain("Hidden + decoy");
    expect(vaultSource).not.toContain("Open hidden volume");
    expect(vaultSource).not.toContain("Open visible decoy");
    expect(vaultSource).toContain("mountPasswordSelectedVolume");
    expect(vaultSource).toContain("The password selects the matching standard, outer, or hidden volume automatically.");
    expect(mountHandlerSource).toContain("readOnly: mountReadOnly");
    expect(vaultSource).toContain("Mount read-only");
    expect(vaultSource).toContain("Off by default.");
    expect(backendSource).toContain("VolumeKind: params.volumeKind");
    expect(backendSource).toContain("VolumeRole: params.volumeRole");
  });

  test("separates driver-only mounts from drives this Windows session can open", () => {
    expect(vaultSource).toContain('volume.accessible !== false');
    expect(vaultSource).toContain("needs attention");
    expect(vaultSource).toContain("Not available in this Windows sign-in");
  });

  test("Secure Storage follows saved mount visibility and verifies the Explorer-facing drive", () => {
    expect(vaultSource).toContain("<MountScopeSelector");
    expect(vaultSource).toContain("appSettings?.app?.vault?.mountScope ?? \"auto\"");
    expect(vaultSource).toContain("resolveEffectiveMountScope(mountScopePreference, systemInfo?.osName)");
    expect(vaultSource).toContain("scope: effectiveMountScope");
    expect(vaultSource).toContain("requestedScope");
    expect(vaultSource).toContain("mountPersonalVolume({");
    expect(vaultSource).toContain("await verifyVaultDrive(result.data.drive)");
    expect(vaultSource).toContain("setMountedVolume(result.data)");
    expect(vaultSource).toContain('icon="warning-sign"');
    expect(sidebarSource).toContain("presentation: quickMountScope");
    expect(sidebarSource).toContain('readOnly: false');
    expect(sidebarSource).toContain("resolveEffectiveMountScope(appSettings?.app?.vault?.mountScope, systemInfo?.osName)");
    expect(sidebarSource).toContain("mountPersonalVolume({");
    expect(sidebarSource).toContain("await verifyVaultDrive(r.drive)");
    expect(backendSource).toContain('invoke<{ drive: string; accessible: boolean }>("verify_vault_drive"');
    expect(appContextSource).toContain("setEncryptionStatus(null);");
    expect(vaultSource).not.toContain("Only this Windows account");
    expect(vaultSource).not.toContain("The drive will not appear in other users’ File Explorer sessions.");
    expect(backendSource).toContain("PresentedLetter: letter");
  });

  test("Secure Storage hides engine-only rows until caller authorization is verified", () => {
    expect(vaultSource).toContain("const serviceAuthorityReady = !mountAuthoritiesLoading && !mountAuthoritiesUnavailable;");
    expect(vaultSource).toContain("volumes.filter((volume) => secureStorageMountIsAuthorized(volume, personalMounts, fleetEntries))");
    expect(vaultSource).toContain("{visibleVolumes.map((vol: any, idx: number) => (");
    expect(vaultSource).toContain("Mounted volumes are hidden until the secure service can verify access.");
    expect(vaultSource).not.toContain("{volumes.map((vol: any, idx: number) => (");
  });

  test("reports when a personal mount has no filesystem ACL attestation", () => {
    expect(backendSource).toContain("aclAttested: boolean");
    expect(vaultSource).toContain("Existing file permissions are preserved");
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
