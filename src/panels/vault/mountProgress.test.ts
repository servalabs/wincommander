import { expect, test } from "bun:test";
import { mountProgressMessage, waitForMountOptions, waitForMountReadback } from "./mountOperationProgress";

test("normal mounts show the current stage even without a custom PIM", () => {
  expect(mountProgressMessage("unlocking", 3, false)).toContain("Vault service");
  expect(mountProgressMessage("checking", 0, false)).toContain("drive letter");
  expect(mountProgressMessage("verifying", 4, false)).toContain("Windows can open");
  expect(mountProgressMessage("refreshing", 5, false)).toContain("mounted-volume list");
});

test("post-mount readback is bounded without claiming a failed or cancelled mount", async () => {
  for (const code of ["vault_mount_readback_unconfirmed", "vault_confirmed_mount_list_unavailable"] as const) {
    expect(await waitForMountReadback(new Promise(() => {}), code, 5).catch(error => error.message)).toBe(code);
    expect(await waitForMountReadback(Promise.resolve({ accessible: true }), code, 20)).toEqual({ accessible: true });
  }
});
test("slow work never claims success or suggests a duplicate mount", () => {
  expect(mountProgressMessage("unlocking", 30, false)).toContain("No result has been confirmed");
  expect(mountProgressMessage("unlocking", 30, true)).toContain("custom PIM");
});
test("read-only option lookups finish or fail with a bounded actionable category", async () => {
  expect(await waitForMountOptions(Promise.resolve(["J"]), 20)).toEqual(["J"]);
  expect(await waitForMountOptions(new Promise(() => {}), 5).catch(error => error.message)).toBe("vault_mount_options_timeout");
  expect(await waitForMountOptions(Promise.reject(new Error("denied")), 20).catch(error => error.message)).toBe("denied");
});
