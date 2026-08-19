// SPDX-License-Identifier: AGPL-3.0-or-later
// src/lib/searchTypeCycle.test.ts
// Tab wrap: null → folders → pdf → excel → word → menu. "menu" is not a ChipKind.

import { describe, expect, it } from "bun:test";
import { CHIP_DEFS } from "./searchTokens";
import type { ChipKind } from "./searchTokens";
import { TAB_TYPE_CYCLE, TYPE_DROPDOWN_ORDER, TYPE_FILTER_MAX_VISIBLE, isTabTypeCycleKind, nextAppendType, nextTabType, visibleSelectedTypes } from "./searchTypeCycle";

describe("TAB_TYPE_CYCLE", () => {
  it("is folders → pdf → excel → word", () => {
    expect([...TAB_TYPE_CYCLE]).toEqual(["folders", "pdf", "excel", "word"]);
  });

  it("contains only ChipKinds", () => {
    for (const kind of TAB_TYPE_CYCLE) {
      expect(CHIP_DEFS.some((def) => def.kind === kind)).toBe(true);
      expect(isTabTypeCycleKind(kind)).toBe(true);
    }
  });
});

describe("nextTabType", () => {
  it("cycles null → folders → pdf → excel → word → menu", () => {
    expect(nextTabType(null)).toBe("folders");
    expect(nextTabType("folders")).toBe("pdf");
    expect(nextTabType("pdf")).toBe("excel");
    expect(nextTabType("excel")).toBe("word");
    expect(nextTabType("word")).toBe("menu");
  });

  it("returns menu after word, and menu is not a ChipKind", () => {
    const next = nextTabType("word");
    expect(next).toBe("menu");
    const chipKinds = new Set<string>(CHIP_DEFS.map((def) => def.kind));
    expect(chipKinds.has(next)).toBe(false);
    expect(isTabTypeCycleKind(next as ChipKind)).toBe(false);
  });

  it("restarts at folders when the current type is outside the Tab cycle", () => {
    expect(nextTabType("videos")).toBe("folders");
    expect(nextTabType("images")).toBe("folders");
    expect(nextTabType("code")).toBe("folders");
  });
});

describe("nextAppendType", () => {
  it("appends folders → pdf → excel → word, then menu", () => {
    expect(nextAppendType([])).toBe("folders");
    expect(nextAppendType(["folders"])).toBe("pdf");
    expect(nextAppendType(["folders", "pdf"])).toBe("excel");
    expect(nextAppendType(["folders", "pdf", "excel"])).toBe("word");
    expect(nextAppendType(["folders", "pdf", "excel", "word"])).toBe("menu");
  });

  it("returns the first missing cycle kind even when a later one is already selected", () => {
    expect(nextAppendType(["pdf"])).toBe("folders");
    expect(nextAppendType(["excel", "word"])).toBe("folders");
    expect(nextAppendType(["folders", "pdf"])).toBe("excel");
    expect(nextAppendType(["folders", "excel", "word"])).toBe("pdf");
  });

  it("returns menu only when all four cycle kinds are present", () => {
    expect(nextAppendType(["folders", "pdf", "excel", "word", "images"])).toBe("menu");
    expect(nextAppendType(["word", "videos"])).toBe("folders");
  });
});

describe("TYPE_DROPDOWN_ORDER", () => {
  it("lists Type menu kinds in priority order", () => {
    expect([...TYPE_DROPDOWN_ORDER]).toEqual([
      "videos",
      "images",
      "slides",
      "text",
      "audio",
      "archives",
      "apps",
      "code",
    ]);
  });
});

describe("visibleSelectedTypes", () => {
  it("caps the control at four selected icons and reports the rest as overflow", () => {
    expect(TYPE_FILTER_MAX_VISIBLE).toBe(4);
    expect(visibleSelectedTypes([])).toEqual({ visible: [], overflow: [] });
    expect(visibleSelectedTypes(["folders", "pdf"])).toEqual({
      visible: ["folders", "pdf"],
      overflow: [],
    });
    expect(visibleSelectedTypes(["folders", "pdf", "excel", "word"])).toEqual({
      visible: ["folders", "pdf", "excel", "word"],
      overflow: [],
    });
    expect(visibleSelectedTypes(["folders", "pdf", "excel", "word", "videos", "images"])).toEqual({
      visible: ["folders", "pdf", "excel", "word"],
      overflow: ["videos", "images"],
    });
  });
});
