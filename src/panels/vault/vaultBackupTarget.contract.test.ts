import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = readFileSync("src/panels/vault/StegoBackupSection.tsx", "utf8");

test("stego backup operations remain behind the paid entitlement gate", () => {
  expect(source).toContain('<TierGate tier="paid" featureLabel="Stego Backup">');
});
