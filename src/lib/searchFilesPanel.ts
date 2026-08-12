import { suggestChip } from "@/lib/searchTokens";
import type { ChipKind } from "@/lib/searchTokens";
import type { DateFilter, SearchType, SizeFilter } from "@/lib/fileNameSearch";

export interface TabFilterSuggestion {
  kind: ChipKind;
  label: string;
  /** Input value after the recognized keyword has become a visible filter. */
  nextQuery: string;
}

const CHIP_TO_SEARCH_TYPE: Partial<Record<ChipKind, SearchType>> = {
  files: "files",
  folders: "folders",
  documents: "documents",
  images: "images",
  videos: "videos",
  audio: "audio",
  archives: "archives",
  apps: "apps",
  code: "code",
};

/** Return a Tab-accept suggestion that this panel can faithfully express with
 * its existing visible filters.  This keeps typed `folder`, `images`, `today`,
 * and similar words consistent with the Ctrl+Space launcher. */
export function getTabFilterSuggestion(
  query: string,
  searchTypes: Set<SearchType>,
  size: SizeFilter,
  date: DateFilter,
): TabFilterSuggestion | null {
  const suggestion = suggestChip({ chips: [], text: query });
  if (!suggestion) return null;
  const { kind } = suggestion.chip;
  const type = CHIP_TO_SEARCH_TYPE[kind];
  if (type && !searchTypes.has(type)) {
    return { kind, label: kind === "documents" ? "Docs" : kind[0].toUpperCase() + kind.slice(1), nextQuery: suggestion.nextText };
  }
  if (kind === "big" && size !== "large") return { kind, label: "Over 100 MB", nextQuery: suggestion.nextText };
  if (kind === "small" && size !== "tiny") return { kind, label: "Under 1 MB", nextQuery: suggestion.nextText };
  if (kind === "today" && date !== "today") return { kind, label: "Today", nextQuery: suggestion.nextText };
  if (kind === "thisWeek" && date !== "week") return { kind, label: "This week", nextQuery: suggestion.nextText };
  if (kind === "last30Days" && date !== "month") return { kind, label: "This month", nextQuery: suggestion.nextText };
  return null;
}

export function mergeIndexedRoots(currentRoots: string[], addedRoots: string[]): string[] {
  return Array.from(new Set([...currentRoots, ...addedRoots]));
}

export function removeIndexedRoot(currentRoots: string[], root: string): string[] {
  return currentRoots.filter((candidate) => candidate !== root);
}

export function getIndexDisplayError(lastError: string | null | undefined): string | null {
  if (!lastError) return null;
  if (lastError.startsWith("Extraction error:")) return null;
  return lastError;
}
