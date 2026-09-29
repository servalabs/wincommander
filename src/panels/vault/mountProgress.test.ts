import { expect, test } from "bun:test";
import { mountProgressMessage, waitForMountOptions } from "./mountOperationProgress";

test("normal mounts show the current stage even without a custom PIM", () => {
  expect(mountProgressMessage("unlocking", 3, false)).toContain("Vault service");
  expect(mountProgressMessage("checking", 0, false)).toContain("drive letter");
  expect(mountProgressMessage("verifying", 4, false)).toContain("Windows can open");
  expect(mountProgressMessage("refreshing", 5, false)).toContain("mounted-volume list");
});
test("slow work never claims success or suggests a duplicate mount", () => {
  expect(mountProgressMessage("unlocking", 30, false)).toContain("No result has been confirmed");
  expect(mountProgressMessage("unlocking", 30, true)).toContain("custom PIM");
  expect(mountProgressMessage("permission", 40, false)).toContain("your decision");
});
test("read-only option lookups finish or fail with a bounded actionable category", async () => {
  expect(await waitForMountOptions(Promise.resolve(["J"]), 20)).toEqual(["J"]);
  expect(await waitForMountOptions(new Promise(() => {}), 5).catch(error => error.message)).toBe("vault_mount_options_timeout");
  expect(await waitForMountOptions(Promise.reject(new Error("denied")), 20).catch(error => error.message)).toBe("denied");
});
