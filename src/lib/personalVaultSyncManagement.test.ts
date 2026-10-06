import { describe, expect, it } from "bun:test";
import { DEFAULT_VAULT_SYNC_LABEL, validVaultSyncLabel, validateVaultSyncDrafts, vaultSyncLabel, acceptsVaultSyncMountReceipt } from "./personalVaultSyncManagement";
describe("personal Vault sync choices", () => {
  it("does not transfer an open dialog's consent to a replacement in the same drive slot", () => {
    const first="a".repeat(64), replacement="b".repeat(64);
    expect(acceptsVaultSyncMountReceipt("",first)).toBe(true);
    expect(acceptsVaultSyncMountReceipt(first,first)).toBe(true);
    expect(acceptsVaultSyncMountReceipt(first,replacement)).toBe(false);
    for (const invalid of [null,undefined,"", "x".repeat(64)]) expect(acceptsVaultSyncMountReceipt("",invalid)).toBe(false);
  });
  it("keeps folder labels independent of local paths", () => {
    expect(DEFAULT_VAULT_SYNC_LABEL).toBe("WinCommander Vault");
    expect(validateVaultSyncDrafts([{ path: "Phone\\Camera", label: "My phone photos" }], []).ok).toBe(true);
    expect(validVaultSyncLabel("Camera\nPhone")).toBe(false);
    expect(validVaultSyncLabel("é".repeat(65))).toBe(false);
    expect(validVaultSyncLabel(" ")).toBe(false);
    expect(vaultSyncLabel()).toBe(DEFAULT_VAULT_SYNC_LABEL);
    expect(vaultSyncLabel("   ")).toBe(DEFAULT_VAULT_SYNC_LABEL);
    expect(validateVaultSyncDrafts([{ path: "Phone\\Camera", label: " " }], []).ok).toBe(true);
  });
  it("checks new roots against existing roots before any enrollment", () => {
    expect(validateVaultSyncDrafts([{ path: "Phone\\Camera", label: "Photos" }], ["Phone"]).ok).toBe(false);
    expect(validateVaultSyncDrafts([{ path: "Phone\\Docs", label: "Docs" }], ["Phone\\Camera"]).ok).toBe(true);
    expect(validateVaultSyncDrafts([{ path: "Phone\\Camera", label: "Photos" }], ["phone/camera"]).ok).toBe(false);
  });
});
