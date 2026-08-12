export interface LocalLoginUser {
  name: string;
  fullName: string;
  description?: string | null;
  enabled: boolean;
  hiddenFromLogin: boolean;
  builtIn: boolean;
  currentUser?: boolean;
  sid?: string | null;
}

export function filterAndOrderLocalUsers(
  rows: readonly LocalLoginUser[],
  query: string,
): LocalLoginUser[] {
  const q = query.trim().toLowerCase();
  const filtered = q
    ? rows.filter((row) =>
        row.name.toLowerCase().includes(q) ||
        row.fullName.toLowerCase().includes(q) ||
        (row.description ?? "").toLowerCase().includes(q)
      )
    : [...rows];

  return filtered.sort((a, b) => {
    const aRank = [
      a.builtIn ? 1 : 0,
      a.enabled ? 0 : 1,
      a.hiddenFromLogin ? 0 : 1,
      a.name.toLowerCase(),
    ] as const;
    const bRank = [
      b.builtIn ? 1 : 0,
      b.enabled ? 0 : 1,
      b.hiddenFromLogin ? 0 : 1,
      b.name.toLowerCase(),
    ] as const;

    for (let i = 0; i < aRank.length; i++) {
      if (aRank[i] < bRank[i]) return -1;
      if (aRank[i] > bRank[i]) return 1;
    }
    return 0;
  });
}
