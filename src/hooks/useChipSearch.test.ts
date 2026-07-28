// SPDX-License-Identifier: AGPL-3.0-or-later
// src/hooks/useChipSearch.test.ts
//
// Coverage for FIX-B: the ranking comparator was the single largest source of
// search failure (MEASURED over 2073 real queries: recall@10 44.8% vs
// recall@300 70.6%; folders worst of all at recall@10 8.6% vs recall@300
// 66.9%). appSortScore is module-private in production, so it is exported
// here as the test seam (see useChipSearch.ts) rather than mocking the Tauri
// `invoke` IPC boundary through the whole hook — every other hook test in
// this codebase (useManagedPolicy.test.ts, useActivePanelPoller.test.ts, …)
// tests an exported pure function directly for the same reason.
//
// What must NOT regress: an app-shaped query still opens the app first
// (brave.exe / Brave.lnk beats an exact-match non-app file, which beats a
// merely prefix-matching app, which beats an unrelated app, which beats an
// unrelated folder, which beats an unrelated file). See the JUDGEMENT CALL
// comment on appSortScore in useChipSearch.ts for the reasoning.

import { describe, expect, it } from "bun:test";
import { appSortScore, isMatchTermTooShort, MIN_MATCH_TERM_LENGTH } from "./useChipSearch";
import type { SearchResult } from "../lib/fileNameSearch";

function file(name: string, dir = "C:\\Users\\Admin\\Documents"): SearchResult {
  return { name, directory: dir, full_path: `${dir}\\${name}`, size: "1024", modified: "" };
}
function folder(name: string, dir = "C:\\Users\\Admin\\Documents"): SearchResult {
  return { name, directory: dir, full_path: `${dir}\\${name}`, size: "", modified: "" };
}

/** Sorts a mixed bag the same way fetchMatches's byScore does for the tier
 *  term — frecency/pathPreference tie-breaks are exercised separately from
 *  the hook, so this only asserts tier order. */
function byTier(term: string) {
  return (a: SearchResult, b: SearchResult) => appSortScore(a, term) - appSortScore(b, term);
}

describe("appSortScore — the eight tiers", () => {
  it("tier 0: exact basename match on an app extension", () => {
    expect(appSortScore(file("brave.exe"), "brave")).toBe(0);
    expect(appSortScore(file("Brave.lnk"), "brave")).toBe(0); // case-insensitive
  });

  it("tier 1: exact basename match on ANY other type — folders and plain files included", () => {
    expect(appSortScore(folder("assets"), "assets")).toBe(1);
    expect(appSortScore(file("budget.txt"), "budget")).toBe(1);
    expect(appSortScore(file("brave.svg"), "brave")).toBe(1);
  });

  it("tier 2: prefix basename match on an app extension", () => {
    expect(appSortScore(file("BraveUpdateSetup.exe"), "brave")).toBe(2);
  });

  it("tier 3: prefix basename match on ANY other type", () => {
    expect(appSortScore(folder("Assets Archive 2019"), "assets")).toBe(3);
    expect(appSortScore(file("budget-report.txt"), "budget")).toBe(3);
  });

  it("tier 4: any .lnk with no name relation at all", () => {
    expect(appSortScore(file("Spotify.lnk"), "budget")).toBe(4);
  });

  it("tier 5: any other app binary with no name relation at all", () => {
    expect(appSortScore(file("notepad.exe"), "budget")).toBe(5);
    expect(appSortScore(file("installer.msi"), "budget")).toBe(5);
  });

  it("tier 6: a directory with no name relation at all — the folder-burying fix", () => {
    expect(appSortScore(folder("Downloads"), "budget")).toBe(6);
  });

  it("tier 7: everything else — a plain file with no name relation", () => {
    expect(appSortScore(file("notes.txt"), "budget")).toBe(7);
  });

  it("an empty term can never register as a prefix match (fetchMatches never calls with one, but the function must not lie if it is)", () => {
    expect(appSortScore(file("anything.txt"), "")).toBe(7);
    expect(appSortScore(folder("anything"), "")).toBe(6);
  });
});

describe("appSortScore — hidden/dotfile folders get a basename, not an empty string", () => {
  // MEASURED: this codebase's own corpus of real folders is dominated by
  // hidden/config directories shaped exactly like this — .claude-plugin,
  // .cargo, .bin, .claude, .git — and the sharpest case in the whole task
  // (`…\ralph-loop\.claude-plugin` ranking 96th) is one of them. Without this
  // fix, nameWithoutExt(".claude-plugin") returns "" and the exact/prefix
  // tiers above silently never engage for any hidden folder at all.
  it("an exact query for a hidden folder's full name gets tier 1, not tier 6/7", () => {
    expect(appSortScore(folder(".claude-plugin"), ".claude-plugin")).toBe(1);
    expect(appSortScore(folder(".cargo"), ".cargo")).toBe(1);
  });

  it("a prefix query for a hidden folder still gets tier 3", () => {
    expect(appSortScore(folder(".claude-plugin"), ".claude")).toBe(3);
  });

  it("a REAL multi-dot extension is unaffected — only a name whose ONLY dot leads is special-cased", () => {
    expect(appSortScore(file("readme.txt"), "readme")).toBe(1); // unchanged existing behaviour
    expect(appSortScore(file(".env.local"), ".env")).toBe(1); // strips the real trailing extension
    // The literal full name is NOT an exact match here (tier 7, not 1) — same
    // pre-existing behaviour any multi-dot name has always had ("archive.tar.gz"
    // vs "archive.tar" is likewise not "exact"); this fix only special-cases a
    // name whose ONLY dot is the leading one.
    expect(appSortScore(file(".env.local"), ".env.local")).toBe(7);
  });
});

describe("appSortScore — folders are no longer buried behind noise (headline fix)", () => {
  it("an exact-match folder outranks 300 unrelated files sharing its tier before the fix", () => {
    const target = folder("ralph-loop");
    const noise = Array.from({ length: 300 }, (_, i) => file(`unrelated-${i}.dat`));
    const ranked = [...noise, target].sort(byTier("ralph-loop"));
    expect(ranked[0]).toBe(target);
  });

  it("a prefix-match folder outranks unrelated noise the same way an exact match does", () => {
    const target = folder("Assets Archive 2019");
    const noise = Array.from({ length: 50 }, (_, i) => file(`unrelated-${i}.dat`));
    const ranked = [...noise, target].sort(byTier("assets"));
    expect(ranked[0]).toBe(target);
  });

  it("a NON-matching folder still beats a NON-matching plain file (tier 6 vs 7)", () => {
    const ranked = [file("notes.txt"), folder("Downloads")].sort(byTier("budget"));
    expect(ranked[0].name).toBe("Downloads");
  });
});

describe("appSortScore — app priority is preserved exactly where it was measured to matter", () => {
  it("REGRESSION: an app-shaped query still opens the app first, even with exact-match non-app files of the same name present", () => {
    // Real corpus shape: brave.exe/.lnk coexists with brave.json, brave.svg,
    // a prefix-matching installer, an unrelated app, and an unrelated folder.
    const braveApp = file("Brave.lnk");
    const braveJson = file("brave.json");
    const braveSvg = file("brave.svg");
    const braveInstaller = file("BraveUpdateSetup.exe");
    const unrelatedApp = file("Discord.exe");
    const unrelatedFolder = folder("Downloads");
    const unrelatedFile = file("notes.txt");

    const ranked = [
      unrelatedFile, unrelatedFolder, unrelatedApp, braveInstaller, braveSvg, braveJson, braveApp,
    ].sort(byTier("brave"));

    expect(ranked[0]).toBe(braveApp); // tier 0
    expect(ranked.slice(1, 3)).toContain(braveJson); // tier 1 (order vs braveSvg unspecified)
    expect(ranked.slice(1, 3)).toContain(braveSvg); // tier 1
    expect(ranked[3]).toBe(braveInstaller); // tier 2
    expect(ranked[4]).toBe(unrelatedApp); // tier 5 (no lnk present at tier 4 here)
    expect(ranked[5]).toBe(unrelatedFolder); // tier 6
    expect(ranked[6]).toBe(unrelatedFile); // tier 7
  });

  it("an app with ZERO name relation still beats a non-matching folder or file — unchanged invariant", () => {
    const ranked = [folder("Downloads"), file("notes.txt"), file("notepad.exe")].sort(byTier("budget"));
    expect(ranked[0].name).toBe("notepad.exe");
  });

  it("an exact non-app match still beats an app that only PREFIX-matches (the judgement call)", () => {
    // "chrome" the folder vs ChromeSetup.exe: exact intent beats a partial
    // substring hit on an installer — see the JUDGEMENT CALL comment.
    const exactFolder = folder("chrome");
    const prefixApp = file("ChromeSetup.exe");
    const ranked = [prefixApp, exactFolder].sort(byTier("chrome"));
    expect(ranked[0]).toBe(exactFolder);
  });
});

describe("appSortScore — the merge fix: a hit exclusive to one fetched page is never stranded", () => {
  it("a low-tier item found ONLY in a second page is not buried behind an already-sorted first page", () => {
    // Reproduces the shape of the sorted/unsorted dual fetch in fetchMatches:
    // combine into ONE pool, sort ONCE — never sort each page separately and
    // concatenate, which would strand an old file recovered only by the
    // unsorted page behind the entire sorted page regardless of its tier.
    const sortedPage = Array.from({ length: 300 }, (_, i) => file(`recent-${i}.dat`));
    const oldExactFolder = folder("ralph-loop"); // exclusive to the unsorted page
    const unsortedPage = [oldExactFolder];

    const pool = [...sortedPage, ...unsortedPage];
    const ranked = pool.sort(byTier("ralph-loop"));
    expect(ranked[0]).toBe(oldExactFolder);
  });

  it("documents the anti-pattern this replaces: sorting each page separately then concatenating buries the hit by POSITION, not tier", () => {
    const sortedPage = Array.from({ length: 300 }, (_, i) => file(`recent-${i}.dat`));
    const oldExactFolder = folder("ralph-loop");
    const unsortedPage = [oldExactFolder];
    const wrongShape = [
      ...[...sortedPage].sort(byTier("ralph-loop")),
      ...[...unsortedPage].sort(byTier("ralph-loop")),
    ];
    // Every sortedPage row is tier 7 here, yet all 300 of them still precede
    // the unsorted page's tier-1 folder — proving position beat tier under
    // the old shape, which is exactly what MATCH_SHOW=10 would then cut off.
    expect(wrongShape.indexOf(oldExactFolder)).toBe(300);
  });
});

describe("isMatchTermTooShort — the timeout gate", () => {
  it("MEASURED: 1-2 character terms are refused (56% timeout rate under load)", () => {
    expect(isMatchTermTooShort("a")).toBe(true);
    expect(isMatchTermTooShort("ab")).toBe(true);
  });

  it("a 3+ character term is allowed through", () => {
    expect(isMatchTermTooShort("abc")).toBe(false);
    expect(isMatchTermTooShort("budget report")).toBe(false);
    expect(MIN_MATCH_TERM_LENGTH).toBe(3);
  });

  it("never gates the empty term — that is a browse, not a short match, and must stay reachable", () => {
    expect(isMatchTermTooShort("")).toBe(false);
  });
});
