import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const vaultStyles = readFileSync("src/panels/vault/index.css", "utf8");
const stegoSource = readFileSync("src/panels/vault/StegoBackupSection.tsx", "utf8");
const stegoStyles = readFileSync("src/panels/vault/StegoBackupSection.css", "utf8");
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

  test("Stego Backup uses a wrapped information popover without a warning icon", () => {
    expect(stegoSource).toContain("<StegoInfoPopover content={INFO.what} />");
    expect(stegoSource).toContain('popoverClassName="stego-info-popover"');
    expect(stegoSource).toContain('aria-label="Stego Backup information"');
    expect(stegoSource).not.toContain("Important stego backup warning");
    expect(stegoSource).not.toContain('icon="warning-sign"');
    expect(stegoStyles).toContain(".stego-info-popover");
    expect(stegoStyles).toContain("white-space: normal");
    expect(stegoStyles).toContain("overflow-wrap: anywhere");
    expect(stegoStyles).toContain("max-width: min(320px, calc(100vw - 24px))");
  });

  test("the final Security & Apps wipe card has breathing room before its divider", () => {
    expect(tweaksSource).toContain('gridClassName={noSearch ? "tweaks-security-apps-toggle-grid" : undefined}');
    expect(tweaksStyles).toContain(".tweaks-security-apps-toggle-grid");
    expect(tweaksStyles).toContain("margin-bottom: 8px");
  });
});
