import { describe, expect, test } from "bun:test";
import { patchAuthorizedEntriesFromMountResult } from "./vaultAccessUiState";

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
