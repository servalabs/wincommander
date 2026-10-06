import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/components/RightSidebar.tsx", "utf8");

describe("Fleet Vault quick mount", () => {
  test("requires an authorized receipt before trusting even an empty mount readback", () => {
    expect(source).toContain("confirmedBulkDismountMessage(receipt, observed)");
    expect(source).not.toContain("if (!observed) throw new Error('vault_dismount_readback_unconfirmed')");
  });
  test("uses the caller-filtered service list and opaque entry ids", () => {
    expect(source).toContain("listAuthorizedEntries, mountEntry: mountFleetVaultEntry");
    expect(source).toContain("fleetVaultRefresh(() => listAuthorizedEntries(), background)");
    expect(source).toContain("mountFleetVaultEntry(fleetVaultEntryId, fleetVaultPassword, 'outer')");
    expect(source).toContain("key={entry.entry_id} value={entry.entry_id}");
  });

  test("refreshes visible status and supersedes older reads after bulk cleanup", () => {
    expect(source).toContain("createVaultStatusRefresh<FleetQuickMountEntry[]>");
    expect(source).toContain("window.setInterval(refreshVisible, 20_000)");
    expect(source).toContain("window.addEventListener('focus', refreshVisible)");
    expect(source).toContain("Promise.all([refreshVault(true), refreshFleetVaults()])");
    expect(source).toContain("fleetVaultsUnavailable ?");
  });

  test("refreshes its service-filtered list after a saved Fleet Vault changes", async () => {
    const hook = readFileSync("src/hooks/useVaultAccess.ts", "utf8");
    expect(hook).toContain('invoke<Status>("apply_vault_owner_policy_fragment", { policy: fragment })');
    expect(hook).toContain('afterVaultMutation(() => invoke<Status>("apply_vault_owner_policy_fragment"');
    expect(source).toContain('window.addEventListener(FLEET_VAULTS_CHANGED_EVENT, refreshAfterFleetVaultSave)');
    expect(source).toContain('window.removeEventListener(FLEET_VAULTS_CHANGED_EVENT, refreshAfterFleetVaultSave)');
  });

  test("opens on saved Fleet Vaults without requiring a personal shortcut", () => {
    const openHandler = source.slice(source.indexOf("const handleQmOpen"), source.indexOf("const patchQmSlots"));

    expect(openHandler).toContain("setQmEditing(null)");
    expect(openHandler).not.toContain("quickMountSlots.length === 0");
    expect(source).toContain("entry.drive_letter ?? entry.preferred_letter");
    expect(source).toContain("Loading your saved Fleet Vaults");
  });

  test("does not disclose saved Fleet paths through the sidebar dropdown", () => {
    const fleetStart = source.indexOf('className="qm-fleet-vaults"');
    const fleetSection = source.slice(fleetStart, source.indexOf('{quickMountSlots.length === 0', fleetStart));
    expect(fleetSection).toContain("Container paths are never exposed here.");
    expect(fleetSection).not.toContain("container_path");
    expect(fleetSection).not.toContain("filePath");
    expect(fleetSection).not.toContain("mountVolume({");
  });
});
