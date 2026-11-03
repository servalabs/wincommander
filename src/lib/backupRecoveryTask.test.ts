import { describe, expect, test } from "bun:test";
import { backupFailure, createBackupTaskStore, parseBackupResult } from "./backupRecoveryTask";
const id = "00000000-0000-4000-8000-000000000001";
const copied = { operation: "copied", bytes: "19", directoryName: `WinCommander-Container-Copy-${id}`, legacy: false };

describe("explicit backup task lifecycle", () => {
  test("copy-check results retain the submitted subject and receipt time", async () => {
    const receivedAt = 1_800_000_000_000;
    const store = createBackupTaskStore(() => receivedAt);
    const context = { checkedCopy: "D:/retrieved-first" };
    let resolve!: (value: { success: boolean; data: unknown }) => void;
    const pending = new Promise<{ success: boolean; data: unknown }>((done) => { resolve = done; });
    const check = store.run("checked", () => pending, context);
    context.checkedCopy = "D:/selected-later";
    resolve({ success: true, data: { operation: "checked", bytes: "19", legacy: false, checkedCopy: "not-the-submitted-folder", receivedAt: -1 } });
    expect(await check).toBe(true);
    expect(store.getSnapshot().checkedCopy).toBe("D:/retrieved-first");
    expect(store.getSnapshot().receivedAt).toBe(receivedAt);
    expect(createBackupTaskStore().getSnapshot().state).toBe("idle");
  });
  test("a new failed or unknown check cannot retain a previous successful observation", async () => {
    for (const response of [{ success: false, error: "backup_rejected: invalid receipt" }, { success: true, data: {} }]) {
      const store = createBackupTaskStore(() => 1_800_000_000_000);
      await store.run("checked", async () => ({ success: true, data: { operation: "checked", bytes: "19", legacy: false } }), { checkedCopy: "D:/first" });
      await store.run("checked", async () => response, { checkedCopy: "D:/second" });
      expect(store.getSnapshot().state).not.toBe("done");
      expect(store.getSnapshot().checkedCopy).toBe("D:/second");
      expect(store.getSnapshot().receivedAt).toBeUndefined();
      expect(store.getSnapshot().result).toBeUndefined();
    }
  });
  test("a check without a selected subject is not dispatched or presented as success", async () => {
    const store = createBackupTaskStore(); let calls = 0;
    expect(await store.run("checked", async () => { calls++; return { success: true }; })).toBe(false);
    expect(calls).toBe(0);
    expect(store.getSnapshot().state).toBe("failed");
  });
  test("duplicate clicks do not replay; panel resubscription observes the same running operation", async () => {
    const store = createBackupTaskStore();
    let resolve!: (value: { success: boolean; data: unknown }) => void;
    let calls = 0;
    const pending = new Promise<{ success: boolean; data: unknown }>((done) => { resolve = done; });
    const first = store.run("copied", () => { calls++; return pending; }, { destinationParent: "\\\\server\\share\\" });
    expect(store.getSnapshot().state).toBe("running");
    const unsubscribe = store.subscribe(() => {}); unsubscribe();
    expect(await store.run("copied", () => { calls++; return pending; })).toBe(false);
    expect(calls).toBe(1);
    let observed = "";
    store.subscribe(() => { observed = store.getSnapshot().state; });
    resolve({ success: true, data: copied });
    expect(await first).toBe(true);
    expect(observed).toBe("done");
    expect(store.getSnapshot().destination).toBe(`\\\\server\\share\\${copied.directoryName}`);
  });
  test("unknown IPC outcome stays unknown without any automatic second call", async () => {
    const store = createBackupTaskStore(); let calls = 0;
    await store.run("copied", async () => { calls++; return { success: false, error: "backup_outcome_unknown; request was not replayed" }; });
    expect(calls).toBe(1); expect(store.getSnapshot().state).toBe("unknown");
    expect(store.getSnapshot().message).toContain("not replayed");
  });
  test("a drive-root destination retains its separator", async () => {
    const root = "C:" + String.fromCharCode(92);
    const store = createBackupTaskStore();
    await store.run("copied", async () => ({ success: true, data: copied }), { destinationParent: root });
    expect(store.getSnapshot().destination).toBe(root + copied.directoryName);
  });
  test("a success flag without a valid matching receipt cannot become success", async () => {
    for (const data of [undefined, {}, { ...copied, operation: "restored" }, { ...copied, directoryName: "..\\external" }, { ...copied, bytes: "18446744073709551616" }, { ...copied, files: 1 }]) {
      const store = createBackupTaskStore();
      await store.run("copied", async () => ({ success: true, data }));
      expect(store.getSnapshot().state).toBe("unknown");
      expect(store.getSnapshot().result).toBeUndefined();
    }
  });
  test("unexpected transport exceptions retain an unknown outcome and destination", async () => {
    const store = createBackupTaskStore(); let calls = 0;
    await store.run("restored", async () => { calls++; throw new Error("untrusted transport body"); }, { destinationParent: "C:\\private" });
    expect(calls).toBe(1);
    expect(store.getSnapshot().state).toBe("unknown");
    expect(store.getSnapshot().destinationParent).toBe("C:\\private");
    expect(store.getSnapshot().message).not.toContain("untrusted");
  });
  test("legacy and older sidecar errors are actionable, untrusted errors are not displayed", () => {
    expect(backupFailure("backup_legacy_required").message).toContain("explicitly");
    expect(backupFailure("feature_unknown").message).toContain("Update WinCommander Pro");
    expect(backupFailure("backup_rejected: recovery destination must be outside the source generation").message).toContain("outside the selected backup");
    expect(backupFailure("backup_rejected: cannot verify destination ancestry").message).toContain("parent read permissions");
    expect(backupFailure("C:\\private-secret\\untrusted-body").message).not.toContain("private-secret");
    expect(parseBackupResult({ operation: "restored", bytes: "0", files: 0, legacy: true, directoryName: `WinCommander-Restored-Files-${id}` }, "restored")?.legacy).toBe(true);
    expect(parseBackupResult({ operation: "checked", bytes: "1", legacy: true }, "checked")).toBeNull();
  });
});
