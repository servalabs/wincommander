import { describe, expect, test } from "bun:test";
import { canResumeProUpdate, clearPendingProUpdate, handoffFreeUpdate, readPendingProUpdate, rememberPendingProUpdate } from "./pendingProUpdate";

function storage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
  };
}

describe("Pro continuation across a Windows installer exit", () => {
  test("persists before the installer can exit the process and keeps the request for relaunch", async () => {
    const disk = storage();
    let installed = false;
    await handoffFreeUpdate(disk, "3.6.5", true, async () => true, async () => {
      expect(readPendingProUpdate(disk)?.freeVersion).toBe("3.6.5");
      installed = true;
    });
    expect(installed).toBe(true);
    expect(canResumeProUpdate(readPendingProUpdate(disk), "3.6.5")).toBe(true);
  });

  test("an installer handoff failure cancels its pending continuation", async () => {
    const disk = storage();
    let error = "";
    try {
      await handoffFreeUpdate(disk, "3.6.5", true, async () => true, async () => { throw new Error("setup failed"); });
    } catch (caught) { error = String(caught); }
    expect(error).toContain("setup failed");
    expect(readPendingProUpdate(disk)).toBeNull();
  });

  test("never schedules a first Pro install for a Free update", async () => {
    for (const entitled of [true, false]) {
      const disk = storage();
      let installed = false;
      await handoffFreeUpdate(disk, "3.6.5", entitled, async () => false, async () => { installed = true; });
      expect(installed).toBe(true);
      expect(readPendingProUpdate(disk)).toBeNull();
    }
  });

  test("failed continuation persistence prevents a misleading partial combined update", async () => {
    let installed = false;
    let error = "";
    try {
      await handoffFreeUpdate({ ...storage(), setItem: () => { throw new Error("storage denied"); } },
        "3.6.5", true, async () => true, async () => { installed = true; });
    } catch (caught) { error = String(caught); }
    expect(error).toContain("Nothing was installed");
    expect(installed).toBe(false);
  });

  test("survives process exit and resumes only after the requested Free version actually starts", () => {
    const disk = storage();
    rememberPendingProUpdate(disk, "3.6.5");
    const restarted = readPendingProUpdate(disk);
    expect(canResumeProUpdate(restarted, "3.6.4")).toBe(false);
    expect(canResumeProUpdate(restarted, "3.6.5")).toBe(true);
    expect(canResumeProUpdate(restarted, "3.6.6")).toBe(false);
  });

  test("completion or failure cannot erase another window's newer request", () => {
    const disk = storage();
    rememberPendingProUpdate(disk, "3.6.6");
    clearPendingProUpdate(disk, "3.6.5");
    expect(readPendingProUpdate(disk)?.freeVersion).toBe("3.6.6");
    clearPendingProUpdate(disk, "3.6.6");
    expect(readPendingProUpdate(disk)).toBeNull();
  });

  test("expired, future-dated, malformed and inaccessible state never requests installation", () => {
    const disk = storage();
    rememberPendingProUpdate(disk, "3.6.5", 0);
    expect(readPendingProUpdate(disk, 8 * 86400000)).toBeNull();
    rememberPendingProUpdate(disk, "3.6.5", 100);
    expect(readPendingProUpdate(disk, 99)).toBeNull();
    disk.setItem("wincommander.pendingProUpdate", "not JSON");
    expect(readPendingProUpdate(disk)).toBeNull();
    expect(readPendingProUpdate({ ...disk, getItem: () => { throw new Error("Access denied"); } })).toBeNull();
    let rejected = false;
    try { rememberPendingProUpdate(disk, "not-a-version"); } catch { rejected = true; }
    expect(rejected).toBe(true);
  });
});
