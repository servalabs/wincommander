import { describe, expect, test } from "bun:test";
import type { EncryptionStatus } from "@/hooks/useBackend";
import type { VaultSyncManagementResult } from "@/types/generated/ipc";
import { inspectPostMountSync } from "./vaultSyncPostMount";

const receipt = "a".repeat(64);
const notice = { drive: "P:", warning: "recovery_required" as const };
const status = (mount = "b") => ({ success: true, data: { volumes: [{ letter: "P:", path: "disposable-container", internalDrive: 8,
  accessible: true, browseAllowed: true, quickMountReceipt: `qm1:${mount.repeat(64)}` }] } as EncryptionStatus });
const listed = (recovery = true): VaultSyncManagementResult => ({ managed: true, mount_receipt: receipt, syncthing_enabled: true,
  can_enable_syncthing: false, gui_url: null, removed: false,
  folders: [{ folder_id: "folder", relative_path: "Phone\\Camera", label: "Camera", paused: recovery, recovery_required: recovery }] });
const roots = [{ relative_path: "Phone\\Camera", reason: "marker_missing" as const, token: "c".repeat(64) }];
async function failureOf(pending: Promise<unknown>): Promise<string> {
  try { await pending; } catch (failure) { return String(failure); }
  throw new Error("Expected the unverifiable operation to fail");
}

describe("background post-mount sync inspection", () => {
  test("waits off-dialog and binds recovery inspection to the freshly verified mount receipt", async () => {
    let resolve!: (value: VaultSyncManagementResult) => void;
    const pending = new Promise<VaultSyncManagementResult>(done => { resolve = done; });
    const calls: unknown[] = [];
    const check = inspectPostMountSync(notice, {
      readStatus: async () => status(), list: () => pending,
      inspect: async (slot, expected) => { calls.push([slot, expected]); return { enabled: false, gui_url: "", recovery_required: true, recovery_roots: roots }; },
    });
    await Promise.resolve();
    expect(calls).toEqual([]);
    resolve(listed());
    const result = await check;
    expect(result.kind).toBe("recovery");
    if (result.kind !== "recovery") throw new Error("recovery choices required");
    expect(result.notice).toEqual({ ...notice, recovery: { internalDrive: 8, roots } });
    expect(result.target.receipt).toBe(`qm1:${"b".repeat(64)}`);
    expect(calls).toEqual([[8, receipt]]);
  });
  test("never inspects a changed, absent, inaccessible, or unowned mount", async () => {
    for (const invalid of [null, { volumes: [] }, { volumes: [{ ...status().data.volumes?.[0], browseAllowed: false }] }]) {
      let calls = 0;
      expect(await failureOf(inspectPostMountSync(notice, { readStatus: async () => ({ success: true, data: invalid as EncryptionStatus | null }),
        list: async () => { calls++; return listed(); }, inspect: async () => { throw new Error("must not inspect"); } }))).toContain("vault_mount_state_unknown");
      expect(calls).toBe(0);
    }
    let reads = 0;
    const result = await inspectPostMountSync(notice, { readStatus: async () => status(reads++ ? "d" : "b"), list: async () => listed(),
      inspect: async () => { throw new Error("must not inspect replacement"); } });
    expect(result.kind).toBe("stale");
  });
  test("suppresses late inspection results after remount and never turns an invalid recovery response into success", async () => {
    let reads = 0;
    const api = { readStatus: async () => status(++reads === 3 ? "d" : "b"), list: async () => listed(),
      inspect: async () => ({ enabled: false, gui_url: "", recovery_required: true, recovery_roots: roots }) };
    expect((await inspectPostMountSync(notice, api)).kind).toBe("stale");
    expect(await failureOf(inspectPostMountSync(notice, { ...api, readStatus: async () => status(),
      inspect: async () => ({ enabled: false, gui_url: "", recovery_required: true }) }))).toContain("vault_broker_rejected");
  });
  test("reports no recovery decisions without enrolling, recreating, or resuming folders", async () => {
    expect(await inspectPostMountSync(notice, { readStatus: async () => status(), list: async () => listed(false),
      inspect: async () => { throw new Error("healthy folders must not enroll"); } })).toEqual({ kind: "clear" });
  });
  test("unverified lists and transport failures remain failures without a retry mutation", async () => {
    for (const listing of [async () => { throw new Error("service reply timed out"); }, async () => ({ ...listed(), mount_receipt: null }),
      async () => ({ ...listed(), syncthing_enabled: false })]) {
      let inspected = 0;
      await failureOf(inspectPostMountSync(notice, { readStatus: async () => status(), list: listing,
        inspect: async () => { inspected++; throw new Error("must not inspect unverifiable listing"); } }));
      expect(inspected).toBe(0);
    }
  });
});
