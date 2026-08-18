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

  test("does not bring Ink Receipt or Clipboard Guard into main", () => {
    expect(panel).not.toContain("Clipboard Guard");
    expect(panel).not.toContain("Ink Receipt");
    expect(panel).not.toContain("FleetAdminGate");
  });

  test("copies a non-secret local manifest", () => {
    expect(vault).toContain("contains no plaintext passwords");
  });
});
