import { describe, expect, test } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";

const panel = readFileSync("src/panels/fleet/index.tsx", "utf8");
const vault = readFileSync("src/panels/fleet/VaultAccessTab.tsx", "utf8");
const consoleLink = readFileSync("src/panels/fleet/FleetConsoleLink.tsx", "utf8");
const fleetFiles = readdirSync("src/panels/fleet", { withFileTypes: true }).map(entry => entry.name);

describe("native Fleet Vault panel contracts", () => {
  test("keeps enrollment and adds a local Vault-access tab", () => {
    expect(panel).toContain("Enrollment");
    expect(panel).toContain("Vault access");
    expect(panel).toContain("<FleetConnectView />");
    expect(panel).toContain("<VaultAccessTab />");
  });

  test("hands organization policy administration to the Fleet server console", () => {
    expect(panel).toContain("<FleetConsoleLink />");
    expect(consoleLink).toContain("Open Fleet console");
    expect(consoleLink).toContain('url.protocol === "http:" || url.protocol === "https:"');
    expect(panel).not.toContain("FleetAdminSession");
    expect(panel).not.toContain("ClipboardGuardTab");
    expect(panel).not.toContain("InkReceiptTab");
  });

  test("does not ship desktop organization-policy editors or an admin API client", () => {
    for (const file of [
      "ClipboardGuardTab.tsx",
      "ClipboardRuleEditor.tsx",
      "InkReceiptTab.tsx",
      "FleetAdminSession.tsx",
      "fleetAdminApi.ts",
      "fleetAdminTypes.ts",
    ]) {
      expect(fleetFiles).not.toContain(file);
    }
  });

  test("copies a non-secret local manifest", () => {
    expect(vault).toContain("contains no plaintext passwords");
  });
});
