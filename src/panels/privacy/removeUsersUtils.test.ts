import { describe, it, expect } from "bun:test";
import { isUserSelectable, toggleUser } from "./removeUsersUtils";
import type { LocalLoginUser } from "../../components/tweaks/managers/localUsersManagerUtils";

function makeUser(overrides: Partial<LocalLoginUser> = {}): LocalLoginUser {
  return {
    name: "alice",
    fullName: "Alice",
    enabled: true,
    hiddenFromLogin: false,
    builtIn: false,
    currentUser: false,
    ...overrides,
  };
}

describe("isUserSelectable", () => {
  it("is not selectable when built-in", () => {
    expect(isUserSelectable(makeUser({ builtIn: true }))).toBe(false);
  });

  it("is not selectable when it's the currently signed-in account", () => {
    expect(isUserSelectable(makeUser({ currentUser: true }))).toBe(false);
  });

  it("is not selectable when both built-in and current", () => {
    expect(isUserSelectable(makeUser({ builtIn: true, currentUser: true }))).toBe(false);
  });

  it("is selectable for a normal, non-current account", () => {
    expect(isUserSelectable(makeUser())).toBe(true);
  });
});

describe("toggleUser", () => {
  it("adds the name when absent", () => {
    expect(toggleUser([], "alice")).toEqual(["alice"]);
  });

  it("removes the name when present", () => {
    expect(toggleUser(["alice"], "alice")).toEqual([]);
  });

  it("preserves other names when adding", () => {
    expect(toggleUser(["bob"], "alice")).toEqual(["bob", "alice"]);
  });

  it("preserves other names when removing", () => {
    expect(toggleUser(["bob", "alice", "carol"], "alice")).toEqual(["bob", "carol"]);
  });

  it("matches names case-sensitively", () => {
    expect(toggleUser(["Alice"], "alice")).toEqual(["Alice", "alice"]);
  });
});
