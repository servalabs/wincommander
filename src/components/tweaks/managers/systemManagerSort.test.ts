import { describe, expect, test } from "bun:test";
import { enabledRowsFirst } from "./systemManagerSort";

describe("system manager sorting", () => {
  test("puts enabled rows first while preserving order inside each group", () => {
    const rows = [
      { name: "disabled-a", enabled: false },
      { name: "enabled-a", enabled: true },
      { name: "disabled-b", enabled: false },
      { name: "enabled-b", enabled: true },
    ];

    expect(enabledRowsFirst(rows, (row) => row.enabled).map((row) => row.name)).toEqual([
      "enabled-a",
      "enabled-b",
      "disabled-a",
      "disabled-b",
    ]);
  });

  test("treats non-disabled service start modes as enabled", () => {
    const rows = [
      { name: "dead", startMode: "Disabled" },
      { name: "manual", startMode: "Manual" },
      { name: "auto", startMode: "Automatic" },
    ];

    expect(enabledRowsFirst(rows, (row) => row.startMode !== "Disabled").map((row) => row.name)).toEqual([
      "manual",
      "auto",
      "dead",
    ]);
  });
});
