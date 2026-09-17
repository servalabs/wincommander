import { describe, expect, test } from "bun:test";
import { canOpenFleetNavigation } from "./fleetNavigationAccess";

describe("Fleet navigation access", () => {
  test("is available only to the elevated administrator process", () => {
    expect(canOpenFleetNavigation(true)).toBe(true);
    expect(canOpenFleetNavigation(false)).toBe(false);
  });

  test("fails closed while the process token is not known", () => {
    expect(canOpenFleetNavigation(null)).toBe(false);
    expect(canOpenFleetNavigation(undefined)).toBe(false);
  });
});
