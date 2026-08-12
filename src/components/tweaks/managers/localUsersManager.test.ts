import { describe, expect, test } from "bun:test";
import { filterAndOrderLocalUsers } from "./localUsersManagerUtils";

const rows = [
  { name: "svc-video", fullName: "", enabled: true, hiddenFromLogin: false, builtIn: false },
  { name: "Administrator", fullName: "", enabled: false, hiddenFromLogin: true, builtIn: true },
  { name: "ops-bot", fullName: "Ops Bot", enabled: true, hiddenFromLogin: true, builtIn: false },
  { name: "Guest", fullName: "", enabled: false, hiddenFromLogin: false, builtIn: true },
];

describe("local users manager", () => {
  test("puts regular enabled accounts before built-in and disabled accounts", () => {
    expect(filterAndOrderLocalUsers(rows, "").map((row) => row.name)).toEqual([
      "ops-bot",
      "svc-video",
      "Administrator",
      "Guest",
    ]);
  });

  test("filters by account name and full name", () => {
    expect(filterAndOrderLocalUsers(rows, "ops").map((row) => row.name)).toEqual(["ops-bot"]);
    expect(filterAndOrderLocalUsers(rows, "video").map((row) => row.name)).toEqual(["svc-video"]);
  });
});
