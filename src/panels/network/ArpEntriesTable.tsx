// The neighbour-mapping rows. Presentation only — sorting, filtering and the
// duplicate-MAC heuristic all live in src/lib/arpDiagnostics.ts.
import { useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Icon } from "../../components/ui/icon";
import {
  arpMacKey,
  arpRowKey,
  findSuspiciousMacs,
  sortArpEntries,
  type ArpSortKey,
  type SortDirection,
} from "../../lib/arpDiagnostics";
import type { ArpEntry } from "../../hooks/useBackend";
import { InfoTip } from "./InfoTip";
import { SortHeader } from "./MaintenanceNotice";

const DYNAMIC_TIP =
  "Learned automatically when this PC talked to the device. Safe to clear — Windows re-learns it on the next packet.";
const STATIC_TIP =
  "Configured by Windows itself (multicast and broadcast plumbing). Clearing never touches these.";
const SUSPICIOUS_TIP =
  "This hardware address is answering for more than one IP on the same adapter. Normal for a router using proxy-ARP, but also what ARP spoofing looks like.";

export function ArpEntriesTable({ entries }: { entries: ArpEntry[] }) {
  // Default order groups rows by adapter and then walks the subnet numerically,
  // which is how someone actually reads a neighbour table.
  const [sortKey, setSortKey] = useState<ArpSortKey>("interface");
  const [direction, setDirection] = useState<SortDirection>("asc");
  const [query, setQuery] = useState("");

  const suspicious = useMemo(() => findSuspiciousMacs(entries), [entries]);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const filtered = needle
      ? entries.filter(
          (e) =>
            e.address.toLowerCase().includes(needle) ||
            e.physicalAddress.toLowerCase().includes(needle) ||
            e.interface.toLowerCase().includes(needle),
        )
      : entries;
    return sortArpEntries(filtered, sortKey, direction);
  }, [entries, query, sortKey, direction]);

  const toggleSort = (key: ArpSortKey) => {
    if (key === sortKey) setDirection((d) => (d === "asc" ? "desc" : "asc"));
    else {
      setSortKey(key);
      setDirection("asc");
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <label className="flex items-center gap-2 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-2.5 py-1.5">
        <Icon icon="search" size={13} className="shrink-0 text-[var(--text-mute)]" />
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter by IP, MAC or adapter"
          className="w-full bg-transparent font-[family-name:var(--font-mono)] text-[12px] text-[var(--text)] outline-none placeholder:font-[family-name:var(--font-sans)] placeholder:text-[var(--text-mute)]"
        />
        {query ? (
          <button
            type="button"
            aria-label="Clear filter"
            onClick={() => setQuery("")}
            className="shrink-0 text-[var(--text-mute)] hover:text-[var(--text)]"
          >
            <Icon icon="cross" size={13} />
          </button>
        ) : null}
      </label>

      <div className="wc-net-table-scroll">
        <table className="wc-net-table">
          <thead>
            <tr>
              <SortHeader
                label="IP address"
                active={sortKey === "address"}
                direction={direction}
                onClick={() => toggleSort("address")}
                className="wc-net-col-ip"
              />
              <SortHeader
                label="MAC address"
                active={sortKey === "physicalAddress"}
                direction={direction}
                onClick={() => toggleSort("physicalAddress")}
                className="wc-net-col-mac"
                info={
                  <InfoTip
                    label="What a MAC address is"
                    content="The device's permanent hardware address on this network segment. Windows calls it the physical address."
                  />
                }
              />
              <SortHeader
                label="Via adapter"
                active={sortKey === "interface"}
                direction={direction}
                onClick={() => toggleSort("interface")}
                className="wc-net-col-adapter"
                info={
                  <InfoTip
                    label="What the adapter column shows"
                    content="The local IP of the network adapter that holds this entry. A PC with Wi-Fi and Ethernet keeps a separate neighbour table per adapter."
                  />
                }
              />
              <SortHeader
                label="Type"
                active={sortKey === "entryType"}
                direction={direction}
                onClick={() => toggleSort("entryType")}
                className="wc-net-col-type"
              />
            </tr>
          </thead>
          <tbody>
            {visible.map((entry) => {
              const flagged = suspicious.has(arpMacKey(entry));
              const isDynamic = entry.entryType.toLowerCase() === "dynamic";
              return (
                <tr key={arpRowKey(entry)} className={flagged ? "is-flagged" : undefined}>
                  <td className="wc-net-mono">{entry.address}</td>
                  <td className="wc-net-mono">
                    <span className="inline-flex items-center gap-1.5">
                      {entry.physicalAddress}
                      {flagged ? (
                        <InfoTip label="Why this row is flagged" content={SUSPICIOUS_TIP} />
                      ) : null}
                    </span>
                  </td>
                  <td className="wc-net-mono wc-net-dim">{entry.interface}</td>
                  <td>
                    {/* Dynamic is the NORMAL state, so it gets the neutral tone.
                        The previous amber-for-dynamic colouring implied the
                        opposite of the truth. */}
                    <InfoTip
                      label={isDynamic ? "What dynamic means" : "What static means"}
                      content={isDynamic ? DYNAMIC_TIP : STATIC_TIP}
                    />
                    <Badge tone={isDynamic ? "neutral" : "accent"} className="ml-1 align-middle">
                      {isDynamic ? "learned" : entry.entryType}
                    </Badge>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {visible.length === 0 ? (
        <p className="py-3 text-center text-[12px] text-[var(--text-mute)]">
          No entry matches “{query}”.
        </p>
      ) : (
        <p className="text-[11px] text-[var(--text-mute)]">
          Showing {visible.length} of {entries.length} {entries.length === 1 ? "entry" : "entries"}.
        </p>
      )}
    </div>
  );
}
