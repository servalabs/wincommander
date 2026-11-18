import { describe, expect, it } from "bun:test";
import { isSyncthingSetupUrl, vaultSyncRecoveryChoice, vaultSyncRecoveryReason } from "./vaultSyncRecovery";
import { notifyVaultSyncRecovery, VAULT_SYNC_WARNING_EVENT } from "./vaultSyncWarning";

const roots = [{ relative_path: "Phone/Photos", reason: "root_missing", token: "a".repeat(64) }];

describe("Vault sync recovery choices", () => {
  it("preserves each independently bound folder without exposing paths outside the Vault", () => {
    const observed = vaultSyncRecoveryChoice(8, [...roots, { relative_path: "Calls", reason: "marker_missing", token: "b".repeat(64) }]);
    expect(observed?.internalDrive).toBe(8);
    expect(observed?.roots.map(root => root.relative_path)).toEqual(["Phone/Photos", "Calls"]);
    for (const path of ["", "../Photos", "C:\\Photos", "\\Photos", "Photos//nested", "Photos\u0000"]) {
      expect(vaultSyncRecoveryChoice(8, [{ ...roots[0], relative_path: path }])).toBeNull();
    }
  });
  it("refuses incomplete, duplicated or unbounded confirmation targets", () => {
    for (const slot of [-1, 26, 1.5, "8", undefined]) expect(vaultSyncRecoveryChoice(slot, roots)).toBeNull();
    for (const value of [null, [], [roots[0], roots[0]], [{ ...roots[0], token: "" }], [{ ...roots[0], token: "z".repeat(64) }], [{ ...roots[0], token: "a".repeat(65) }], [{ ...roots[0], reason: "raw error" }]]) {
      expect(vaultSyncRecoveryChoice(8, value)).toBeNull();
    }
  });
  it("routes an enrollment choice through the existing mount dialog event", () => {
    const target = new EventTarget();
    let detail: unknown;
    target.addEventListener(VAULT_SYNC_WARNING_EVENT, event => { detail = (event as CustomEvent).detail; });
    notifyVaultSyncRecovery("j", 8, { enabled: false, gui_url: "http://127.0.0.1:8385", recovery_required: true,
      recovery_roots: roots as NonNullable<Parameters<typeof notifyVaultSyncRecovery>[2]["recovery_roots"]> }, target);
    expect(detail).toEqual({ drive: "J:", warning: "recovery_required", recovery: { internalDrive: 8, roots } });
  });
  it("does not turn a missing safety marker into a claim that files were restored", () => {
    expect(vaultSyncRecoveryReason("marker_missing")).toContain("safety marker");
    expect(vaultSyncRecoveryReason("configuration_missing")).toContain("configuration");
    expect(vaultSyncRecoveryReason("confirmation_required")).toContain("decision");
  });
  it("opens only the verified loopback Syncthing setup location", () => {
    expect(isSyncthingSetupUrl("http://127.0.0.1:51995")).toBe(true);
    for (const value of ["https://127.0.0.1:8385", "http://example.com:8385", "http://127.0.0.1:0", "http://token@127.0.0.1:8385", "http://127.0.0.1:8385/private"]) {
      expect(isSyncthingSetupUrl(value)).toBe(false);
    }
  });
});
