import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const vaultStyles = readFileSync("src/panels/vault/index.css", "utf8");
const tweaksSource = readFileSync("src/panels/tweaks/index.tsx", "utf8");
const tweaksStyles = readFileSync("src/panels/tweaks/index.css", "utf8");

describe("Secure Storage and Windows Settings layouts", () => {
  test("Encrypted Volumes and RAM Disks stretch evenly while their volume areas fill spare room", () => {
    const gridRule = vaultStyles.match(/\.vault-volumes-ramdisks-grid\s*\{([^}]+)\}/)?.[1] ?? "";

    expect(gridRule).toContain("align-items: stretch");
    expect(vaultStyles).toContain(".vault-volumes-ramdisks-grid > .section-card > .section-collapse");
    expect(vaultStyles).toContain(".vault-volumes-ramdisks-grid > .section-card > .section-collapse > .overflow-hidden");
    expect(vaultStyles).toContain(".vault-volumes-ramdisks-grid .vault-content");
    expect(vaultStyles).toContain("flex: 1 1 auto");
  });

  test("the final Security & Apps wipe card has breathing room before its divider", () => {
    expect(tweaksSource).toContain('gridClassName={noSearch ? "tweaks-security-apps-toggle-grid" : undefined}');
    expect(tweaksStyles).toContain(".tweaks-security-apps-toggle-grid");
    expect(tweaksStyles).toContain("margin-bottom: 8px");
  });
});
