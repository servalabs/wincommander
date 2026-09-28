import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/components/RightSidebar.tsx", "utf8");

describe("Fleet Vault quick mount", () => {
  test("uses the caller-filtered service list and opaque entry ids", () => {
    expect(source).toContain("listAuthorizedEntries, mountEntry: mountFleetVaultEntry");
    expect(source).toContain("const entries = await listAuthorizedEntries()");
    expect(source).toContain("mountFleetVaultEntry(fleetVaultEntryId, fleetVaultPassword, 'outer')");
    expect(source).toContain("key={entry.entry_id} value={entry.entry_id}");
  });

  test("refreshes its service-filtered list after a saved Fleet Vault changes", async () => {
    const hook = readFileSync("src/hooks/useVaultAccess.ts", "utf8");
    expect(hook).toContain('invoke<Status>("apply_vault_owner_policy_fragment", { policy: fragment })');
    expect(hook).toContain('new Event(FLEET_VAULTS_CHANGED_EVENT)');
    expect(source).toContain('window.addEventListener(FLEET_VAULTS_CHANGED_EVENT, refreshAfterFleetVaultSave)');
    expect(source).toContain('window.removeEventListener(FLEET_VAULTS_CHANGED_EVENT, refreshAfterFleetVaultSave)');
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
