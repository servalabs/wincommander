// SPDX-License-Identifier: AGPL-3.0-or-later
import { beforeEach, describe, expect, test } from "bun:test";
import {
  consumeSearchHandoff, getSearchPrivacy, mayRememberSearchPath, mayShowSearchPath,
  pathWithinRoots, rememberSearchHandoff, searchPrivacyLease, setSearchPrivacy,
} from "./searchPrivacy";
import type { ContentPrivacyStatus } from "./searchPrivacy";
import { recordOpen, topPaths } from "./frecency";

const mounted: ContentPrivacyStatus = {
  generation: "mount-1", privateRoots: ["V:\\"], blockedRoots: [],
  volumes: [{ root: "V:\\", state: "ready", message: "Mounted" }], notice: null,
};
const locked: ContentPrivacyStatus = {
  ...mounted, generation: "locked", blockedRoots: ["V:\\"],
  volumes: [{ root: "V:\\", state: "unavailable", message: "Locked" }],
};
let data: Map<string, string>;
beforeEach(() => {
  data = new Map();
  Object.defineProperty(globalThis, "localStorage", { configurable: true, writable: true, value: {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
  } });
  setSearchPrivacy(null);
  consumeSearchHandoff();
});

describe("private volume search lifecycle", () => {
  test("unknown mount state suppresses results and persistent history", () => {
    expect(mayShowSearchPath("V:\\secret.txt")).toBe(false);
    recordOpen("V:\\secret.txt");
    expect(data.size).toBe(0);
    expect(topPaths(10)).toEqual([]);
  });

  test("mounted files are searchable without being persisted as recent paths", () => {
    setSearchPrivacy(mounted);
    expect(mayShowSearchPath("V:\\secret.txt")).toBe(true);
    expect(mayRememberSearchPath("V:\\secret.txt")).toBe(false);
    recordOpen("V:\\secret.txt");
    recordOpen("C:\\ordinary.txt");
    expect(topPaths(10)).toEqual(["C:\\ordinary.txt"]);
    expect([...data.values()].join()).not.toContain("secret");
  });

  test("recognizing a private volume removes its old history while preserving ordinary history", () => {
    setSearchPrivacy({ ...mounted, privateRoots: [], volumes: [] });
    recordOpen("V:\\legacy-secret.txt");
    recordOpen("C:\\ordinary.txt");
    setSearchPrivacy(mounted);
    expect(topPaths(10)).toEqual(["C:\\ordinary.txt"]);
    expect([...data.values()].join()).not.toContain("legacy-secret");
  });

  test("locked and reused drive letters cannot synthesize remembered private rows", () => {
    setSearchPrivacy(locked);
    expect(mayShowSearchPath("V:\\secret.txt")).toBe(false);
    expect(mayShowSearchPath("C:\\ordinary.txt")).toBe(true);
    recordOpen("V:\\replacement-drive-file.txt");
    expect(topPaths(10)).toEqual([]);
  });

  test("dismount rejects a preview response already in flight", async () => {
    setSearchPrivacy(mounted);
    const live = searchPrivacyLease();
    let deliver!: (value: string) => void;
    const request = new Promise<string>((resolve) => { deliver = resolve; });
    const accepted = request.then((text) => live() ? text : null);
    setSearchPrivacy(locked);
    deliver("sensitive extracted text");
    expect(await accepted).toBeNull();
  });

  test("remount and status failures never revive an old response lease", () => {
    setSearchPrivacy(mounted);
    const old = searchPrivacyLease();
    setSearchPrivacy(locked);
    setSearchPrivacy({ ...mounted, generation: "mount-2" });
    expect(old()).toBe(false);
    const current = searchPrivacyLease();
    expect(current()).toBe(true);
    setSearchPrivacy(null);
    expect(current()).toBe(false);
  });

  test("indexing progress does not invalidate an otherwise unchanged mount", () => {
    setSearchPrivacy(mounted);
    const current = searchPrivacyLease();
    const revision = getSearchPrivacy().revision;
    setSearchPrivacy({ ...mounted, volumes: [{ root: "V:\\", state: "indexing", message: "Indexing" }] });
    expect(getSearchPrivacy().revision).toBe(revision);
    expect(current()).toBe(true);
  });

  test("root policy handles Windows aliases and segment boundaries", () => {
    expect(pathWithinRoots("\\\\?\\v:\\Folder\\secret.txt", ["V:/"])).toBe(true);
    expect(pathWithinRoots("V:\\Folder\\..\\secret.txt", ["V:/"])).toBe(true);
    expect(pathWithinRoots("C:\\vault-other\\file", ["C:\\vault"])).toBe(false);
    expect(pathWithinRoots("C:\\ordinary\\..\\vault\\file", ["C:\\vault"])).toBe(true);
  });

  test("query handoff stays in memory and removes the legacy disk value", () => {
    data.set("wincommander.search-files-query", "old private query");
    rememberSearchHandoff("secret phrase");
    expect(data.has("wincommander.search-files-query")).toBe(false);
    expect(consumeSearchHandoff()).toBe("secret phrase");
    expect(consumeSearchHandoff()).toBeNull();
  });

  test("mount transitions discard pending query handoffs", () => {
    setSearchPrivacy(mounted);
    rememberSearchHandoff("secret phrase");
    setSearchPrivacy(locked);
    expect(consumeSearchHandoff()).toBeNull();
  });
});
