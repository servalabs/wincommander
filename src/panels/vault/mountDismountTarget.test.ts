import { describe, expect, test } from "bun:test";
import type { PersonalVaultMountedVolume, VaultAuthorizedEntry } from "@/panels/fleet/vaultAccessTypes";
import { secureStorageDismountTarget, secureStorageMountIsAuthorized } from "./mountDismountTarget";

const personalMount: PersonalVaultMountedVolume = {
  drive_letter: "V:",
  internal_drive: 4,
  presentation: "machine",
  cleanup_required: false,
};

const fleetEntry = (can_dismount: boolean): VaultAuthorizedEntry => ({
  entry_id: "fleet-shared-1",
  label: "Shared Vault",
  access: "write",
  presentation: "machine",
  container_kind: "standard",
  mount_state: "mounted",
  drive_letter: "V",
  internal_drive: 4,
  can_dismount,
});

describe("Secure Storage dismount route resolution", () => {
  test("routes an authenticated personal ledger match to its service slot", () => {
    expect(secureStorageDismountTarget({ letter: "V:", internalDrive: 4 }, [personalMount], [])).toEqual({
      kind: "personal",
      internalDrive: 4,
    });
  });

  test("routes an authorized Fleet match by entry id, never the engine slot", () => {
    expect(secureStorageDismountTarget({ letter: "V:", internalDrive: 4 }, [personalMount], [fleetEntry(true)])).toEqual({
      kind: "fleet",
      entryId: "fleet-shared-1",
    });
  });

  test("fails closed for a Fleet mount without service dismount authority", () => {
    expect(secureStorageDismountTarget({ letter: "V:", internalDrive: 4 }, [personalMount], [fleetEntry(false)])).toBeNull();
  });

  test("does not treat an unknown, mismatched, or ambiguous engine row as personal", () => {
    expect(secureStorageDismountTarget({ letter: "X:", internalDrive: 4 }, [personalMount], [])).toBeNull();
    expect(secureStorageDismountTarget({ letter: "V:", internalDrive: 5 }, [personalMount], [])).toBeNull();
    expect(secureStorageDismountTarget({ letter: "V:", internalDrive: 4 }, [personalMount, personalMount], [])).toBeNull();
    expect(secureStorageDismountTarget({ letter: "V:", internalDrive: 4 }, [personalMount], [fleetEntry(false), fleetEntry(true)])).toBeNull();
  });

  test("requires the engine row to match the service-owned internal slot", () => {
    expect(secureStorageDismountTarget({ letter: "V:" }, [personalMount], [])).toBeNull();
    expect(secureStorageDismountTarget({ letter: "V:", internalDrive: 5 }, [], [fleetEntry(true)])).toBeNull();
    expect(secureStorageDismountTarget({ letter: "V:", internalDrive: 4 }, [], [{ ...fleetEntry(true), internal_drive: null }])).toBeNull();
    expect(secureStorageMountIsAuthorized({ letter: "V:" }, [personalMount], [])).toBe(false);
    expect(secureStorageMountIsAuthorized({ letter: "V:", internalDrive: 4 }, [], [{ ...fleetEntry(true), internal_drive: 5 }])).toBe(false);
  });
});

describe("Secure Storage mount row privacy", () => {
  test("shows a Fleet mount only when it appears in this caller's service-authorized list", () => {
    const row = { letter: "V:", internalDrive: 4 };
    expect(secureStorageMountIsAuthorized(row, [], [fleetEntry(false)])).toBe(true);
    expect(secureStorageMountIsAuthorized(row, [], [])).toBe(false);
  });

  test("shows this caller's personal ledger mount, but not engine-only or mismatched rows", () => {
    expect(secureStorageMountIsAuthorized({ letter: "V:", internalDrive: 4 }, [personalMount], [])).toBe(true);
    expect(secureStorageMountIsAuthorized({ letter: "X:", internalDrive: 4 }, [personalMount], [])).toBe(false);
    expect(secureStorageMountIsAuthorized({ letter: "V:", internalDrive: 5 }, [personalMount], [])).toBe(false);
    expect(secureStorageMountIsAuthorized({ letter: "V:", internalDrive: 4 }, [], [])).toBe(false);
  });

  test("hides duplicate or cross-ledger identity collisions", () => {
    expect(secureStorageMountIsAuthorized({ letter: "V:", internalDrive: 4 }, [personalMount, personalMount], [])).toBe(false);
    expect(secureStorageMountIsAuthorized({ letter: "V:", internalDrive: 4 }, [personalMount], [fleetEntry(true)])).toBe(false);
    expect(secureStorageMountIsAuthorized({ letter: "V:", internalDrive: 4 }, [], [fleetEntry(true), fleetEntry(false)])).toBe(false);
  });
});
