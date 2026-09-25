// SPDX-License-Identifier: AGPL-3.0-or-later
// Per-WebView, memory-only policy. Unknown mount state fails closed.
export interface ContentPrivacyStatus {
  generation: string;
  privateRoots: string[];
  blockedRoots: string[];
  volumes: { root: string; state: "ready" | "read_only" | "unavailable" | "indexing"; message: string }[];
  notice: string | null;
}

let snapshot: { status: ContentPrivacyStatus | null; revision: number } = { status: null, revision: 0 };
const listeners = new Set<() => void>();
export const getSearchPrivacy = () => snapshot;
export function subscribeSearchPrivacy(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function setSearchPrivacy(status: ContentPrivacyStatus | null): void {
  if (JSON.stringify(status) === JSON.stringify(snapshot.status)) return;
  const changed = status?.generation !== snapshot.status?.generation
    || JSON.stringify(status?.privateRoots) !== JSON.stringify(snapshot.status?.privateRoots)
    || JSON.stringify(status?.blockedRoots) !== JSON.stringify(snapshot.status?.blockedRoots);
  if (changed) handoffQuery = null;
  snapshot = { status, revision: snapshot.revision + (changed ? 1 : 0) };
  for (const listener of listeners) listener();
}

function normalize(path: string): string {
  let value = path.trim().replace(/\\/g, "/").toLowerCase();
  if (value.startsWith("//?/unc/")) value = `//${value.slice(8)}`;
  else if (value.startsWith("//?/")) value = value.slice(4);
  const prefix = value.startsWith("//") ? "//" : "";
  const segments: string[] = [];
  for (const segment of value.split("/")) {
    if (!segment || segment === ".") continue;
    if (segment === "..") { if (segments.length > 1) segments.pop(); }
    else segments.push(segment);
  }
  return prefix + segments.join("/");
}

export function pathWithinRoots(path: string, roots: string[]): boolean {
  const value = normalize(path);
  return roots.some((root) => {
    const base = normalize(root);
    return value === base || value.startsWith(`${base}/`);
  });
}

export function mayRememberSearchPath(path: string): boolean {
  const status = snapshot.status;
  return !!status && !pathWithinRoots(path, [...status.privateRoots, ...status.blockedRoots]);
}

export function mayShowSearchPath(path: string): boolean {
  const status = snapshot.status;
  return !!status && !pathWithinRoots(path, status.blockedRoots);
}

// Capturing a revision rejects late responses even if a volume is removed and
// remounted before the promise resolves (the backend generation must change).
export function searchPrivacyLease(): () => boolean {
  const revision = snapshot.revision;
  return () => !!snapshot.status && revision === snapshot.revision;
}

const LEGACY_HANDOFF_KEY = "wincommander.search-files-query";
let handoffQuery: string | null = null;
export function rememberSearchHandoff(query: string): void {
  handoffQuery = query;
  try { localStorage.removeItem(LEGACY_HANDOFF_KEY); } catch { /* unavailable storage */ }
}
export function consumeSearchHandoff(): string | null {
  const value = handoffQuery;
  handoffQuery = null;
  try { localStorage.removeItem(LEGACY_HANDOFF_KEY); } catch { /* unavailable storage */ }
  return value;
}
