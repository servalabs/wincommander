import { describe, expect, test } from "bun:test";
import { getCurrentWindowsAccount } from "./windowsAccountIdentity";

describe("current Windows account identity", () => {
  const accounts = [
    { name: "Admin", displayName: "Administrator" },
    { name: "Example", displayName: "Example", isCurrent: true },
  ];

  test("prefers the live current-account marker over a saved/shared name", () => {
    expect(getCurrentWindowsAccount(accounts, "Admin")?.name).toBe("Example");
  });

  test("uses the current service identity when no entry is explicitly marked", () => {
    expect(getCurrentWindowsAccount(accounts.map(({ isCurrent, ...account }) => account), "Admin")?.name).toBe("Admin");
  });
});
