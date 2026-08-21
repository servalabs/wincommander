import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const panel = readFileSync("src/panels/fleet/index.tsx", "utf8");
const access = readFileSync("src/panels/fleet/AccessControlTab.tsx", "utf8");
const vault = readFileSync("src/panels/fleet/VaultAccessTab.tsx", "utf8");
const volumeEditor = readFileSync("src/panels/fleet/VaultVolumesEditor.tsx", "utf8");
const infoPopover = readFileSync("src/panels/fleet/FleetInfoPopover.tsx", "utf8");
const css = readFileSync("src/panels/fleet/index.css", "utf8");
const cleanupBackend = readFileSync("src-tauri/commander-free/scripts/modules/privacy/cleanup.ps1", "utf8");

describe("Fleet access-control panel contracts", () => {
  test("keeps only enrollment, universal access control, and Vault permissions", () => {
    expect(panel).toContain("Enrollment");
    expect(panel).toContain("Access control");
    expect(panel).toContain("Vault permissions");
    expect(panel).not.toContain("FleetConsoleLink");
    expect(panel).not.toContain("ClipboardGuardTab");
    expect(panel).not.toContain("InkReceiptTab");
  });

  test("uses one checkbox directory and sorts selected users first", () => {
    expect(access).toContain('type="checkbox"');
    expect(access).toContain("membershipOrder");
    expect(access).toContain("createAccessGroup");
  });

  test("discovers new Windows accounts before their first sign-in", () => {
    expect(access).toContain("getFleetAccessUsers");
    expect(access).not.toContain("getUserProfiles");
    const fleetDiscovery = cleanupBackend.slice(
      cleanupBackend.indexOf("function Get-FleetAccessUsers"),
      cleanupBackend.indexOf("function Get-LoggedInUsers"),
    );
    expect(fleetDiscovery).toContain("Win32_UserAccount");
    expect(fleetDiscovery).not.toContain("Win32_UserProfile");
  });

  test("keeps a 30/70 layout without nested Access Control scrolling", () => {
    expect(css).toContain("grid-template-columns: minmax(250px, 3fr) minmax(0, 7fr)");
    expect(access).not.toContain("ScrollArea");
    expect(access).not.toContain("Add Windows account name");
    expect(access).not.toContain("addManualUser");
  });

  test("uses the mockup-aligned Vault permissions workspace with per-group permissions", () => {
    expect(volumeEditor).toContain("fleet-vault-workspace");
    expect(volumeEditor).toContain("fleet-vault-matrix");
    expect(volumeEditor).toContain("Owner / administrator");
    expect(volumeEditor).toContain("No access");
    expect(volumeEditor).toContain("Read & write");
    expect(css).toContain("fleet-vault-empty-inline");
    expect(css).toContain("overflow-x: auto");
  });

  test("opens real information popovers for access groups and Vault permissions", () => {
    expect(infoPopover).toContain("PopoverTrigger");
    expect(infoPopover).toContain("PopoverContent");
    expect(access).toContain("How access groups work");
    expect(volumeEditor).toContain("How Vault access is decided");
  });

  test("copies a non-secret local Vault manifest", () => {
    expect(vault).toContain("contains no plaintext passwords");
  });
});
