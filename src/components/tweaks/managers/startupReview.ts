import type { StartupImpactEntry } from "../../../hooks/useBackend";

export interface StartupItem {
  Name: string;
  Command: string;
  RamUsageMB: number;
  Status: string;
  IsEnabled: boolean;
  Source: string;
  Location: string;
  Recommendation: "Keep" | "Disable" | "Neutral";
  Category: string;
  Description: string;
}

export function startupIdentity(item: StartupItem) { return `${item.Location}::${item.Name}`; }

export function startupSignature(item: StartupItem, entries: StartupImpactEntry[]) {
  // Match the actual launcher, never a similar application name.
  return item.Command ? entries.find(entry => entry.command === item.Command) : undefined;
}

export function matchesStartupSearch(item: StartupItem, signature: StartupImpactEntry | undefined, query: string) {
  const q = query.trim().toLowerCase();
  return !q || [item.Name, item.Command, item.Location, item.Description, item.Category, signature?.signer]
    .some(value => value?.toLowerCase().includes(q));
}
