import { expect, test } from "bun:test";
import type { EncryptionStatus } from "@/hooks/useBackend";
import { canOpenQuickMountDrive, quickMountExplorerTarget, quickMountExplorerTargetForDrive, openConfirmedQuickMountTarget } from "./quickMountObservation";

const own = { letter: "S:", path: "D:\\Vault", accessible: true, browseAllowed: true, cleanupRequired: false, internalDrive: 12, quickMountReceipt: `qm1:${"a".repeat(64)}` };
const status = (volumes: unknown[]) => ({ volumes }) as EncryptionStatus;

test("Fleet Quick Mount resolves only one verified owner-session target without renderer paths", () => {
  expect(quickMountExplorerTargetForDrive(status([own]), "s")?.receipt).toBe(own.quickMountReceipt);
  for (const rows of [[], [own, own], [{ ...own, quickMountReceipt: undefined }], [{ ...own, browseAllowed: false }]]) {
    expect(quickMountExplorerTargetForDrive(status(rows), "S:")).toBeNull();
  }
});

test("a cached Quick Mount receipt cannot open an absent, replaced or unverified drive", () => {
  expect(canOpenQuickMountDrive(status([own]), null, "s", "D:\\Vault")).toBe(true);
  expect(canOpenQuickMountDrive(status([]), null, "S:", "D:\\Vault")).toBe(false);
  expect(canOpenQuickMountDrive(status([{ ...own, path: "D:\\Other" }]), null, "S:", "D:\\Vault")).toBe(false);
  expect(canOpenQuickMountDrive(status([{ ...own, accessible: false }]), null, "S:", "D:\\Vault")).toBe(false);
  expect(canOpenQuickMountDrive(status([own]), "unavailable", "S:", "D:\\Vault")).toBe(false);
  expect(canOpenQuickMountDrive(null, null, "S:", "D:\\Vault")).toBe(false);
});

test("Quick Mount opening requires an explicit owner-session receipt, permission and exact observed slot", () => {
  for (const changed of [{ browseAllowed: false }, { browseAllowed: undefined }, { cleanupRequired: true },
    { quickMountReceipt: null }, { quickMountReceipt: "" }, { quickMountReceipt: "unknown-format" }, { internalDrive: undefined }, { internalDrive: 26 }]) {
    expect(quickMountExplorerTarget(status([{ ...own, ...changed }]), null, "S", "D:\\Vault")).toBeNull();
  }
  expect(quickMountExplorerTarget(status([own, own]), null, "S", "D:\\Vault")).toBeNull();
  expect(quickMountExplorerTarget(status([own]), null, "S", "D:\\Vault", 13)).toBeNull();
});

test("auto and manual opening verify Windows readiness then recheck the same receipt before one native open", async () => {
  const target = quickMountExplorerTarget(status([own]), null, "S", "D:\\Vault")!;
  const calls: string[] = [];
  await openConfirmedQuickMountTarget(target, async () => { calls.push("status"); return status([own]); },
    async drive => { calls.push("verify"); return { drive, accessible: true }; },
    async (drive, receipt) => { calls.push("open"); expect(drive).toBe("S:"); expect(receipt).toBe(own.quickMountReceipt); });
  expect(calls).toEqual(["status", "verify", "status", "open"]);
});

test("absent, denied, unready and replaced observations never launch Explorer", async () => {
  const target = quickMountExplorerTarget(status([own]), null, "S", "D:\\Vault")!;
  for (const failure of ["missing", "denied", "windows", "replaced", "different_slot", "read_failure"]) {
    let reads = 0;
    let opened = false;
    let failed = false;
    try {
      await openConfirmedQuickMountTarget(target, async () => {
        reads++;
        if (failure === "missing") return null;
        if (failure === "read_failure") throw new Error("unavailable");
        return status([{ ...own,
          browseAllowed: failure !== "denied",
          quickMountReceipt: failure === "replaced" && reads > 1 ? `qm1:${"b".repeat(64)}` : own.quickMountReceipt,
          internalDrive: failure === "different_slot" ? 13 : own.internalDrive }]);
      }, async drive => ({ drive, accessible: failure !== "windows" }), async () => { opened = true; });
    } catch { failed = true; }
    expect(failed).toBe(true);
    expect(opened).toBe(false);
  }
});

test("an Explorer launch failure does not remount, dismount or retry", async () => {
  const target = quickMountExplorerTarget(status([own]), null, "S", "D:\\Vault")!;
  let opens = 0;
  let failed = false;
  try {
    await openConfirmedQuickMountTarget(target, async () => status([own]), async drive => ({ drive, accessible: true }),
      async () => { opens++; throw new Error("Explorer unavailable"); });
  } catch { failed = true; }
  expect(failed).toBe(true);
  expect(opens).toBe(1);
});
