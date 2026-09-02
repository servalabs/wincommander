import { describe, expect, test } from "bun:test";
import { getCurrentWindowsAccount } from "./windowsAccountIdentity";

describe("current Windows account identity", () => {
  const accounts = [
    { name: "Admin", displayName: "Administrator" },
    { name: "Parth", displayName: "Parth", isCurrent: true },
  ];

  test("prefers the live current-account marker over a saved/shared name", () => {
    expect(getCurrentWindowsAccount(accounts, "Admin")?.name).toBe("Parth");
  });

  test("uses the current service identity when no entry is explicitly marked", () => {
    expect(getCurrentWindowsAccount(accounts.map(({ isCurrent, ...account }) => account), "Admin")?.name).toBe("Admin");
  });
});
