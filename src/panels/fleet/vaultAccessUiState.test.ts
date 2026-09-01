import { describe, expect, test } from "bun:test";
import type { FleetAccessDirectory } from "./accessControlTypes";
import type { VaultAuthorizedEntry } from "./vaultAccessTypes";
import {
  patchAuthorizedEntriesFromMountResult,
  resolveVaultPrincipalOption,
  vaultMountGate,
  vaultPrincipalDirectoryOptions,
  vaultPrincipalSelectOptions,
} from "./vaultAccessUiState";

const entries = [
  {
    entry_id: "shared",
    label: "Shared vault",
    access: "write" as const,
    presentation: "machine" as const,
    container_kind: "standard" as const,
    mount_state: "unmounted" as const,
    drive_letter: null,
  },
  {
    entry_id: "private",
    label: "Private vault",
    access: "read" as const,
    presentation: "per-user" as const,
    container_kind: "standard" as const,
    mount_state: "mounted" as const,
    drive_letter: "V:",
  },
];

describe("patchAuthorizedEntriesFromMountResult", () => {
  test("patches only the returned entry after a successful mount", () => {
    expect(patchAuthorizedEntriesFromMountResult(entries, {
      entry_id: "shared",
      state: "mounted",
      presentation: "machine",
      drive_letter: "S:",
      reason: null,
    })).toEqual([
      { ...entries[0], mount_state: "mounted", drive_letter: "S:" },
      entries[1],
    ]);
  });

  test("clears a stale letter for an unmount without changing other rows", () => {
    expect(patchAuthorizedEntriesFromMountResult(entries, {
      entry_id: "private",
      state: "unmounted",
      presentation: "per-user",
      drive_letter: null,
      reason: null,
    })).toEqual([
      entries[0],
      { ...entries[1], mount_state: "unmounted", drive_letter: null },
    ]);
  });
});

const directory: FleetAccessDirectory = {
  schema: 1,
  users: [
    { id: "sid:1", username: "shrey", displayName: "Shrey Kaushal" },
    { id: "sid:2", username: "partner", isAvailable: false },
  ],
  groups: [
    { id: "grp:1", name: "Sales", localGroup: "WC_Sales", userIds: [] },
  ],
};

describe("vaultPrincipalDirectoryOptions", () => {
  test("emits a group's Windows-resolvable localGroup, not its friendly name", () => {
    const options = vaultPrincipalDirectoryOptions(directory);
    const salesOption = options.find(option => option.kind === "group");
    expect(salesOption).toEqual({ principalName: "WC_Sales", label: "Sales", kind: "group" });
  });

  test("emits a discovered user's bare username, labelled by display name", () => {
    const options = vaultPrincipalDirectoryOptions(directory);
    const userOption = options.find(option => option.kind === "user");
    expect(userOption).toEqual({ principalName: "shrey", label: "Shrey Kaushal", kind: "user" });
  });

  test("excludes a user Windows discovery no longer returns", () => {
    const options = vaultPrincipalDirectoryOptions(directory);
    expect(options.some(option => option.principalName === "partner")).toBe(false);
  });
});

describe("resolveVaultPrincipalOption", () => {
  test("matches a known principal case-insensitively", () => {
    const options = vaultPrincipalDirectoryOptions(directory);
    expect(resolveVaultPrincipalOption("wc_sales", options)).toEqual({
      principalName: "WC_Sales", label: "Sales", kind: "group",
    });
  });

  test("preserves an unknown saved principal as unverified instead of dropping it", () => {
    const options = vaultPrincipalDirectoryOptions(directory);
    expect(resolveVaultPrincipalOption("SERVER\\Sales", options)).toEqual({
      principalName: "SERVER\\Sales", label: "SERVER\\Sales", kind: "unverified",
    });
  });
});

describe("vaultPrincipalSelectOptions", () => {
  test("pins an unresolved saved principal in front of the directory list", () => {
    const options = vaultPrincipalSelectOptions("SERVER\\Sales", directory);
    expect(options[0]).toEqual({ principalName: "SERVER\\Sales", label: "SERVER\\Sales", kind: "unverified" });
    expect(options).toHaveLength(3);
  });

  test("does not pin a principal the directory already resolves", () => {
    const options = vaultPrincipalSelectOptions("WC_Sales", directory);
    expect(options.every(option => option.kind !== "unverified")).toBe(true);
    expect(options).toHaveLength(2);
  });

  test("does not pin a blank grant awaiting a first selection", () => {
    expect(vaultPrincipalSelectOptions("", directory)).toHaveLength(2);
  });
});

function authorizedEntry(overrides: Partial<VaultAuthorizedEntry> = {}): VaultAuthorizedEntry {
  return {
    entry_id: "vault-1",
    label: "Shared vault",
    access: "write",
    presentation: "machine",
    container_kind: "standard",
    mount_state: "unmounted",
    drive_letter: null,
    ...overrides,
  };
}

describe("vaultMountGate", () => {
  test("disables Mount for a dirty draft never applied to the service", () => {
    expect(vaultMountGate({ authorized: undefined, entryResult: undefined, draftDirty: true })).toEqual({
      canMount: false,
      disabledReason: "Save vault settings before mounting — this draft has not been applied yet.",
    });
  });

  test("disables Mount for a saved entry this Windows account isn't granted", () => {
    expect(vaultMountGate({ authorized: undefined, entryResult: undefined, draftDirty: false })).toEqual({
      canMount: false,
      disabledReason: "This Windows account is not authorized to mount this vault.",
    });
  });

  test("disables Mount and surfaces the per-entry failure for a degraded applied entry", () => {
    expect(vaultMountGate({
      authorized: authorizedEntry(),
      entryResult: "principal_resolution_failed",
      draftDirty: false,
    })).toEqual({
      canMount: false,
      disabledReason: "A named Windows user or group could not be resolved",
    });
  });

  test("enables Mount once the entry is authorized and applied cleanly", () => {
    expect(vaultMountGate({ authorized: authorizedEntry(), entryResult: "applied", draftDirty: false }))
      .toEqual({ canMount: true, disabledReason: null });
    expect(vaultMountGate({ authorized: authorizedEntry(), entryResult: undefined, draftDirty: true }))
      .toEqual({ canMount: true, disabledReason: null });
  });
});
