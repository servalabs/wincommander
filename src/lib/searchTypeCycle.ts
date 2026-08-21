// SPDX-License-Identifier: AGPL-3.0-or-later
// src/lib/searchTypeCycle.ts
//
// Tab-cycle kinds (folders, pdf, excel, images) are selected from Type-filter
// icons, not the Type dropdown. Tab APPENDS unused cycle kinds folders → pdf →
// excel → images, then "menu" without dropping already-selected types; after all
// four are present it returns "menu" so the overlay can arm the Type dropdown.
// TYPE_DROPDOWN_ORDER lists only the extra types. "menu" is a sentinel, not a
// ChipKind.

import type { ChipKind } from "./searchTokens";

export const TAB_TYPE_CYCLE = ["folders", "pdf", "excel", "images"] as const satisfies readonly ChipKind[];

export type TabTypeCycleKind = (typeof TAB_TYPE_CYCLE)[number];
export type TabTypeCycleResult = ChipKind | "menu";

export const TYPE_DROPDOWN_ORDER = [
  "word",
  "videos",
  "slides",
  "text",
  "audio",
  "archives",
  "apps",
  "code",
] as const satisfies readonly ChipKind[];

export function isTabTypeCycleKind(kind: ChipKind): kind is TabTypeCycleKind {
  return (TAB_TYPE_CYCLE as readonly ChipKind[]).includes(kind);
}

/** Next Tab-cycle step. Types outside the cycle restart at Folders. */
export function nextTabType(current: ChipKind | null): TabTypeCycleResult {
  if (current === null || !isTabTypeCycleKind(current)) return TAB_TYPE_CYCLE[0];
  const index = TAB_TYPE_CYCLE.indexOf(current);
  if (index >= TAB_TYPE_CYCLE.length - 1) return "menu";
  return TAB_TYPE_CYCLE[index + 1];
}

/** First TAB_TYPE_CYCLE kind not already in `selected`, or "menu" when all four are present. */
export function nextAppendType(selected: readonly ChipKind[]): TabTypeCycleResult {
  const present = new Set(selected);
  for (const kind of TAB_TYPE_CYCLE) {
    if (!present.has(kind)) return kind;
  }
  return "menu";
}

/** Selected type icons on the shortcut Type control. More than this go behind +. */
export const TYPE_FILTER_MAX_VISIBLE = 4;

/** First four selected kinds stay on the control; the rest are overflow. */
export function visibleSelectedTypes(selected: readonly ChipKind[]): {
  visible: ChipKind[];
  overflow: ChipKind[];
} {
  const seen = new Set<ChipKind>();
  const unique: ChipKind[] = [];
  for (const kind of selected) {
    if (seen.has(kind)) continue;
    seen.add(kind);
    unique.push(kind);
  }
  return {
    visible: unique.slice(0, TYPE_FILTER_MAX_VISIBLE),
    overflow: unique.slice(TYPE_FILTER_MAX_VISIBLE),
  };
}
