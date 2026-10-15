import { expect, test } from "bun:test";
import { persistQuickMountExplorerPreference } from "./quickMountExplorerPreference";
import { openQuickMountIfEnabled, quickMountExplorerTarget } from "./quickMountObservation";
import type { EncryptionStatus } from "@/hooks/useBackend";

test("explicit on and off choices persist independently of mount activity", async () => {
  let saved: boolean | undefined;
  const writes: boolean[] = [];
  for (const enabled of [true, false]) {
    await persistQuickMountExplorerPreference(enabled, async value => { saved = value; writes.push(value); }, async () => saved);
    const restarted = JSON.parse(JSON.stringify({ saved })) as { saved: boolean };
    expect(restarted.saved).toBe(enabled);
    const row = { letter: "S:", path: "fixture", accessible: true, browseAllowed: true, internalDrive: 12, quickMountReceipt: `qm1:${"a".repeat(64)}` };
    const status = { volumes: [row] } as EncryptionStatus;
    const target = quickMountExplorerTarget(status, null, "S", "fixture")!;
    let opens = 0;
    await openQuickMountIfEnabled(restarted.saved, target, async () => status,
      async drive => ({ drive, accessible: true }), async () => { opens++; });
    expect(opens).toBe(enabled ? 1 : 0);
    expect(saved).toBe(enabled);
    expect(writes.length).toBe(enabled ? 1 : 2);
  }
  expect(writes).toEqual([true, false]);
});

test("failed persistence or mismatched readback never confirms the checkbox choice", async () => {
  for (const failure of ["write", "read", "missing", "mismatch"]) {
    let confirmed = false;
    try {
      await persistQuickMountExplorerPreference(true, async () => { if (failure === "write") throw new Error("unavailable"); },
        async () => { if (failure === "read") throw new Error("unavailable"); return failure === "missing" ? undefined : false; });
      confirmed = true;
    } catch { /* Expected bounded failure. */ }
    expect(confirmed).toBe(false);
  }
});
