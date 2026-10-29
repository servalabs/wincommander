import { expect, test } from "bun:test";
import { refreshPackageAndAppInventories } from "../panels/apps/packageUpdateDisplay";

test("a package-manager failure is reported without replaying the app inventory", async () => {
  const calls: string[] = [];
  expect(await refreshPackageAndAppInventories(
    async () => { calls.push("apps"); },
    async () => { calls.push("packages"); throw new Error("package check failed"); },
  ).then(() => "resolved", error => error.message)).toBe("package check failed");
  expect(calls).toEqual(["apps", "packages"]);
});
