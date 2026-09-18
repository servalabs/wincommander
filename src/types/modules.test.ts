import { describe, expect, test } from "bun:test";
import { getFirstRunModules, MODULE_DEFS } from "./modules";

describe("first-run module defaults", () => {
  test("enables every declared module before setup has preferences", () => {
    const modules = getFirstRunModules();

    expect(Object.keys(modules).sort()).toEqual(MODULE_DEFS.map(({ id }) => id).sort());
    expect(Object.values(modules)).toEqual(MODULE_DEFS.map(() => true));
  });
});
