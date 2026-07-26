// Firewall audit candidate rules. Presentation only — sorting, filtering and
// the per-rule reading live in src/lib/firewallAuditView.ts.
import { useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Icon } from "../../components/ui/icon";
import {
  filterFirewallRules,
  readFirewallRule,
  sortFirewallRules,
  type FirewallSortKey,
} from "../../lib/firewallAuditView";
import type { SortDirection } from "../../lib/arpDiagnostics";
import type { FirewallRule } from "../../hooks/useBackend";
import { InfoTip } from "./InfoTip";
import { SortHeader } from "./MaintenanceNotice";

const STATE_INFO =
  "What this rule is doing to traffic right now, and whether it is worth acting on. Hover a row's state to see the recommendation.";
const PROGRAM_INFO =
  "The executable the rule applies to. Blank means the rule applies to every program, which makes it the broadest kind of rule.";

export function FirewallRulesTable({
  rules,
  selected,
  disabled,
  onToggle,
  onSetSelection,
}: {
  rules: FirewallRule[];
  selected: Set<string>;
  disabled: boolean;
  onToggle: (id: string) => void;
  onSetSelection: (ids: string[]) => void;
}) {
  // Default order surfaces the rules that actually weaken the firewall first.
  const [sortKey, setSortKey] = useState<FirewallSortKey>("state");
  const [direction, setDirection] = useState<SortDirection>("asc");
  const [query, setQuery] = useState("");

  const visible = useMemo(
    () => sortFirewallRules(filterFirewallRules(rules, query), sortKey, direction),
    [rules, query, sortKey, direction],
  );

  const toggleSort = (key: FirewallSortKey) => {
    if (key === sortKey) setDirection((d) => (d === "asc" ? "desc" : "asc"));
    else {
      setSortKey(key);
      setDirection("asc");
    }
  };

  const allVisibleSelected = visible.length > 0 && visible.every((r) => selected.has(r.id));
  const toggleAllVisible = () => {
    if (allVisibleSelected) {
      const visibleIds = new Set(visible.map((r) => r.id));
      onSetSelection([...selected].filter((id) => !visibleIds.has(id)));
    } else {
      onSetSelection([...new Set([...selected, ...visible.map((r) => r.id)])]);
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex min-w-[220px] flex-1 items-center gap-2 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-2.5 py-1.5">
          <Icon icon="search" size={13} className="shrink-0 text-[var(--text-mute)]" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter by rule name or program"
            className="w-full bg-transparent text-[12px] text-[var(--text)] outline-none placeholder:text-[var(--text-mute)]"
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
        <button
          type="button"
          onClick={toggleAllVisible}
          disabled={disabled || visible.length === 0}
          className="rounded-[var(--r-sm)] border border-[var(--border-strong)] px-2 py-1 text-[11px] text-[var(--text-dim)] transition-colors hover:text-[var(--text)] disabled:opacity-40"
        >
          {allVisibleSelected ? "Deselect shown" : "Select shown"}
        </button>
      </div>

      <div className="wc-net-table-scroll">
        <table className="wc-net-table">
          <thead>
            <tr>
              <th scope="col" className="wc-net-col-check">
                <span className="sr-only">Selected</span>
              </th>
              <SortHeader
                label="Rule"
                active={sortKey === "name"}
                direction={direction}
                onClick={() => toggleSort("name")}
                className="wc-net-col-rule"
              />
              <SortHeader
                label="What it does"
                active={sortKey === "state"}
                direction={direction}
                onClick={() => toggleSort("state")}
                className="wc-net-col-state"
                info={<InfoTip label="What the state column means" content={STATE_INFO} />}
              />
              <SortHeader
                label="Program"
                active={sortKey === "program"}
                direction={direction}
                onClick={() => toggleSort("program")}
                className="wc-net-col-program"
                info={<InfoTip label="What the program column means" content={PROGRAM_INFO} />}
              />
            </tr>
          </thead>
          <tbody>
            {visible.map((rule) => {
              const reading = readFirewallRule(rule);
              const checked = selected.has(rule.id);
              return (
                <tr
                  key={rule.id}
                  className={`wc-net-row-selectable${checked ? " is-selected" : ""}`}
                  onClick={() => !disabled && onToggle(rule.id)}
                >
                  <td>
                    <input
                      type="checkbox"
                      checked={checked}
                      disabled={disabled}
                      aria-label={`Select ${rule.name}`}
                      onChange={() => onToggle(rule.id)}
                      onClick={(e) => e.stopPropagation()}
                      className="size-3.5 accent-[var(--accent)]"
                    />
                  </td>
                  <td className="wc-net-truncate" title={rule.name}>
                    {rule.name}
                  </td>
                  <td>
                    <span className="inline-flex items-center gap-1.5">
                      <Badge tone={reading.tone === "warning" ? "warning" : "neutral"}>
                        {reading.label}
                      </Badge>
                      <InfoTip
                        label={`What to do about ${rule.name}`}
                        content={
                          <span>
                            {reading.meaning}
                            <br />
                            <strong>{reading.advice}</strong>
                          </span>
                        }
                      />
                    </span>
                  </td>
                  <td
                    className="wc-net-mono wc-net-dim wc-net-truncate"
                    title={rule.program || "Applies to every program"}
                  >
                    {rule.program || "all programs"}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {visible.length === 0 ? (
        <p className="py-3 text-center text-[12px] text-[var(--text-mute)]">
          No rule matches “{query}”.
        </p>
      ) : (
        <p className="text-[11px] text-[var(--text-mute)]">
          Showing {visible.length} of {rules.length} {rules.length === 1 ? "rule" : "rules"} ·{" "}
          {selected.size} selected.
        </p>
      )}
    </div>
  );
}
