import { describe, expect, test } from "bun:test";
import { VAULT_NOT_SHARED_FOR_EDIT, vaultPolicyMenuItems, type VaultPolicyMenuInput } from "./vaultPolicyMenu";

const opened: string[] = [];
const input = (overrides: Partial<VaultPolicyMenuInput> = {}): VaultPolicyMenuInput => ({
  fileActions: { changePassword: target => opened.push(`password ${target.path}`), rename: target => opened.push(`rename ${target.path}`) },
  target: { path: "D:\\Vaults\\shared.hc", label: "Shared" },
  mounted: false,
  mountStateUnknown: false,
  saving: false,
  canEditEntry: true,
  canChangeFile: true,
  canRemoveEntry: true,
  canRepairSharedAccess: false,
  onRepair: () => {},
  onForget: () => {},
  onRemove: () => {},
  ...overrides,
});
const labels = (items: ReturnType<typeof vaultPolicyMenuItems>) => items.map(item => item.label);

describe("saved Vault More menu", () => {
  test("holds Change Password, Rename and Remove policy, each once", () => {
    const items = vaultPolicyMenuItems(input());
    expect(labels(items)).toEqual(["Change Password…", "Rename…", "Remove policy"]);
    expect(items.every(item => !item.disabledReason)).toBe(true);
    items[0]!.onSelect();
    items[1]!.onSelect();
    expect(opened.splice(0)).toEqual(["password D:\\Vaults\\shared.hc", "rename D:\\Vaults\\shared.hc"]);
  });

  test("a mounted Vault keeps the items but disables them with the dismount hint", () => {
    const [password, rename, remove] = vaultPolicyMenuItems(input({ mounted: true }));
    expect(password!.disabledReason).toBe("Dismount this vault to change its password or rename it.");
    expect(rename!.disabledReason).toBe(password!.disabledReason);
    expect(remove!.disabledReason).toContain("Dismount");
  });

  test("an account that neither owns nor edits the Vault cannot change or rename it", () => {
    const [password, rename] = vaultPolicyMenuItems(input({ canChangeFile: false, mounted: true }));
    expect(password!.disabledReason).toBe(VAULT_NOT_SHARED_FOR_EDIT);
    expect(rename!.disabledReason).toBe(VAULT_NOT_SHARED_FOR_EDIT);
  });

  test("without Pro only the policy actions remain", () => {
    expect(labels(vaultPolicyMenuItems(input({ fileActions: null })))).toEqual(["Remove policy"]);
  });

  test("repair actions join the menu only when shared access failed", () => {
    const items = vaultPolicyMenuItems(input({ canRepairSharedAccess: true, canEditEntry: false }));
    expect(labels(items)).toEqual(["Change Password…", "Rename…", "Repair shared access", "Forget policy…", "Remove policy"]);
    expect(items[2]!.disabledReason).toContain("cannot repair");
  });
});
