// Pure interpretation helpers for the ARP / neighbour-cache card. No React,
// no IPC — unit tested in arpDiagnostics.test.ts.
//
// WHY this module exists: a raw `arp -a` dump is meaningless to anyone who
// isn't a network engineer. Everything here turns rows into a sentence, and
// flags the one pattern actually worth acting on — a single MAC answering for
// several IPs on one adapter, which is what ARP spoofing looks like.
import type { ArpClearResult, ArpEntry, ArpScan } from "../hooks/useBackend";

/** Mirrors network_maintenance.rs CACHE_TTL. The server refuses a clear
 *  against an older scan, so the UI has to warn before the user hits that. */
export const ARP_SCAN_TTL_MS = 10 * 60 * 1000;

const normMac = (mac: string): string => mac.trim().toLowerCase().replace(/[^0-9a-f]/g, "");

/** IEEE 802 group bit — the LSB of the first octet marks a multicast (or
 *  broadcast) address. Those rows are Windows' own multicast plumbing
 *  (224.0.0.x, 239.x, 255.255.255.255) and always look "duplicated", so they
 *  must never reach the spoof heuristic. */
export function isGroupMac(mac: string): boolean {
  const hex = normMac(mac);
  if (hex.length < 2) return false;
  return (Number.parseInt(hex.slice(0, 2), 16) & 1) === 1;
}

/** Numeric IPv4 ordering so 10.0.0.9 sorts before 10.0.0.10. Anything that
 *  isn't four plain octets falls back to string order. */
export function compareIpv4(a: string, b: string): number {
  const pa = a.split(".");
  const pb = b.split(".");
  if (pa.length !== 4 || pb.length !== 4) return a.localeCompare(b);
  for (let i = 0; i < 4; i += 1) {
    const na = Number(pa[i]);
    const nb = Number(pb[i]);
    if (!Number.isFinite(na) || !Number.isFinite(nb)) return a.localeCompare(b);
    if (na !== nb) return na - nb;
  }
  return 0;
}

export type ArpSortKey = "address" | "physicalAddress" | "interface" | "entryType";
export type SortDirection = "asc" | "desc";

/** Stable row identity. The backend gives no id, and `address` repeats across
 *  adapters, so interface+address is the real composite key (index keys made
 *  React mis-reconcile rows across rescans). */
export const arpRowKey = (entry: ArpEntry): string => `${entry.interface}|${entry.address}`;

/** Key used to look a row up in the `findSuspiciousMacs` result. */
export const arpMacKey = (entry: ArpEntry): string =>
  `${entry.interface}|${normMac(entry.physicalAddress)}`;

export function sortArpEntries(
  entries: ArpEntry[],
  key: ArpSortKey = "address",
  direction: SortDirection = "asc",
): ArpEntry[] {
  const sign = direction === "asc" ? 1 : -1;
  return [...entries].sort((a, b) => {
    let primary = 0;
    if (key === "address") primary = compareIpv4(a.address, b.address);
    else primary = a[key].localeCompare(b[key], undefined, { sensitivity: "base" });
    if (primary !== 0) return primary * sign;
    // Tiebreak is never inverted — it only keeps equal rows in a stable,
    // human-scannable order rather than arp.exe's emission order.
    return a.interface.localeCompare(b.interface) || compareIpv4(a.address, b.address);
  });
}

/** Adapter+MAC pairs where one hardware address answers for more than one
 *  unicast IP on the same adapter. Legitimate for a router doing proxy-ARP,
 *  and also exactly what an ARP-spoofing box looks like — worth surfacing
 *  either way. Group/broadcast MACs are excluded (see isGroupMac). */
export function findSuspiciousMacs(entries: ArpEntry[]): Set<string> {
  const seen = new Map<string, Set<string>>();
  for (const entry of entries) {
    if (isGroupMac(entry.physicalAddress)) continue;
    const key = arpMacKey(entry);
    const addresses = seen.get(key) ?? new Set<string>();
    addresses.add(entry.address);
    seen.set(key, addresses);
  }
  const flagged = new Set<string>();
  for (const [key, addresses] of seen) {
    if (addresses.size > 1) flagged.add(key);
  }
  return flagged;
}

export interface ArpSummary {
  intent: "primary" | "success" | "warning";
  headline: string;
  detail: string;
}

/** The "what does this mean / what do I do" line that turns the table into a
 *  diagnostic. Callers render it as a callout above the rows. */
export function summarizeArpScan(scan: ArpScan): ArpSummary {
  const total = scan.entries.length;
  if (total === 0) {
    return {
      intent: "primary",
      headline: "Nothing cached yet",
      detail:
        "Windows has not talked to any device on this network yet, or every adapter is down. There is nothing to clear.",
    };
  }

  const suspicious = findSuspiciousMacs(scan.entries);
  if (suspicious.size > 0) {
    return {
      intent: "warning",
      headline:
        suspicious.size === 1
          ? "One hardware address is answering for several IPs"
          : `${suspicious.size} hardware addresses are answering for several IPs`,
      detail:
        "Normal if a router on this network answers on behalf of other devices, but it is also what ARP spoofing looks like. Clear the dynamic entries and scan again — if the same pattern returns immediately, treat this network as untrusted.",
    };
  }

  if (scan.dynamicEntries === 0) {
    return {
      intent: "success",
      headline: `${total} ${total === 1 ? "entry" : "entries"}, all static`,
      detail:
        "Static entries are set up by Windows itself and are never cleared, so there is nothing to do here.",
    };
  }

  return {
    intent: "success",
    headline: `${scan.dynamicEntries} of ${total} entries were learned automatically`,
    detail:
      "This is a healthy cache — no action needed. Clearing the learned entries is a safe troubleshooting step when a device is stuck on an old IP: after swapping a router, on a duplicate-IP warning, or when one machine on the network is unreachable while the internet still works.",
  };
}

export interface ErrorAdvice {
  title: string;
  hint: string;
  needsElevation: boolean;
}

/** Maps the raw Rust error strings from network_maintenance.rs onto something
 *  a user can act on. Substring matching, because the backend returns plain
 *  strings with no code — do not tighten these without checking that file. */
export function classifyArpError(raw: string): ErrorAdvice {
  const text = raw.toLowerCase();
  if (text.includes("administrator may be required")) {
    return {
      title: "Windows refused to clear the cache",
      hint: "Clearing neighbour entries needs an elevated process. Restart WinCommander as Administrator and try again.",
      needsElevation: true,
    };
  }
  if (text.includes("could not read the cache") || text.includes("could not run arp.exe")) {
    return {
      title: "Windows would not hand over the neighbour table",
      hint: "arp.exe returned an error. Restart WinCommander as Administrator; if it still fails, the Windows networking stack needs attention.",
      needsElevation: true,
    };
  }
  if (text.includes("expired or is invalid")) {
    return {
      title: "This snapshot is too old to clear from",
      hint: "Scan again — Windows only accepts a clear that matches a scan taken in the last 10 minutes.",
      needsElevation: false,
    };
  }
  if (text.includes("before clearing")) {
    return {
      title: "Scan the cache first",
      hint: "Read the neighbour table before clearing it, so you can see what is about to be removed.",
      needsElevation: false,
    };
  }
  if (text.includes("did not remove any dynamic entries")) {
    return {
      title: "Nothing was removed",
      hint: "The cache is unchanged, which usually means traffic is re-learning entries as fast as they are dropped. Retry, or disconnect the adapter first.",
      needsElevation: false,
    };
  }
  if (text.includes("decoy mode")) {
    return {
      title: "Unavailable in Decoy mode",
      hint: "Decoy mode blocks changes to network state. Leave Decoy mode to clear the cache; reading it stays available.",
      needsElevation: false,
    };
  }
  if (text.includes("investigator mode")) {
    return {
      title: "Blocked by investigator mode",
      hint: "Investigator mode never mutates network state, so evidence stays intact. Reading the neighbour table is still allowed.",
      needsElevation: false,
    };
  }
  if (text.includes("only on windows")) {
    return {
      title: "Windows only",
      hint: "Neighbour-cache maintenance uses the Windows arp.exe utility.",
      needsElevation: false,
    };
  }
  return {
    title: "Neighbour-cache maintenance failed",
    hint: raw,
    needsElevation: false,
  };
}

/** Renders the full before/after picture. `before` is returned by the backend
 *  and was previously dropped on the floor, which lost the "of N" denominator
 *  that makes the number meaningful. */
export function describeArpClear(result: ArpClearResult): string {
  if (result.before === 0) return "There were no learned entries to clear.";
  const tail =
    result.remaining === 0
      ? "None remain."
      : `${result.remaining} ${result.remaining === 1 ? "entry was" : "entries were"} re-learned immediately — that is normal for devices you are actively talking to.`;
  return `Cleared ${result.cleared} of ${result.before} learned ${result.before === 1 ? "entry" : "entries"}. ${tail}`;
}

/** Tab-separated so it pastes straight into a ticket or a spreadsheet. */
export function formatArpTableForClipboard(entries: ArpEntry[]): string {
  const header = "IP ADDRESS\tMAC ADDRESS\tADAPTER\tTYPE";
  const rows = entries.map(
    (e) => `${e.address}\t${e.physicalAddress}\t${e.interface}\t${e.entryType}`,
  );
  return [header, ...rows].join("\n");
}
