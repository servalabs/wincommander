import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const panel = readFileSync("src/panels/fleet/index.tsx", "utf8");
const vault = readFileSync("src/panels/fleet/VaultAccessTab.tsx", "utf8");

describe("native Fleet Vault panel contracts", () => {
  test("keeps enrollment and adds a local Vault-access tab", () => {
    expect(panel).toContain("Enrollment");
    expect(panel).toContain("Vault access");
    expect(panel).toContain("<FleetConnectView />");
    expect(panel).toContain("<VaultAccessTab />");
  });

  test("keeps organization security tabs behind the Fleet-admin gate", () => {
    expect(panel).toContain("Clipboard Guard");
    expect(panel).toContain("Ink Receipt");
    expect(panel).toContain("<FleetAdminGate><ClipboardGuardTab /></FleetAdminGate>");
    expect(panel).toContain("<FleetAdminGate><InkReceiptTab /></FleetAdminGate>");
  });

  test("copies a non-secret local manifest", () => {
    expect(vault).toContain("contains no plaintext passwords");
  });
});
