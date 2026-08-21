export type SortDirection = "ascending" | "descending";

export function filterAndSort<T>(
  items: readonly T[],
  filter: string,
  searchValues: (item: T) => readonly unknown[],
  compare: (left: T, right: T) => number,
  descending: boolean,
): T[] {
  const query = filter.trim().toLocaleLowerCase();
  const filtered = query
    ? items.filter((item) => searchValues(item)
      .some((value) => String(value ?? "").toLocaleLowerCase().includes(query)))
    : [...items];

  return filtered.sort((left, right) => (descending ? -1 : 1) * compare(left, right));
}

export function emptyTableMessage(
  itemName: string,
  total: number,
  filter: string,
): string {
  const query = filter.trim();
  if (total === 0) return `No ${itemName} recorded.`;
  if (query) return `No ${itemName} match “${query}”.`;
  return `No ${itemName} to show.`;
}

export function ariaSort(
  active: boolean,
  descending: boolean,
): "none" | SortDirection {
  if (!active) return "none";
  return descending ? "descending" : "ascending";
}
