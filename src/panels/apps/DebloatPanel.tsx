// src/panels/apps/DebloatPanel.tsx
import { Button, Checkbox, InputGroup } from "@/components/ui/bp";
import { useCallback, useMemo, useState } from "react";
import { runOperation } from "../../context/OperationContext";
import useBackend from "../../hooks/useBackend";
import { showError, showSuccess } from "../../utils/toast";
import { CATEGORY_ORDER } from "./debloatLists";
import { DebloatItem } from "./types";
import { useDebloatInventory } from "./useDebloatInventory";
import ApplicationReviewResults from "./components/ApplicationReviewResults";
import { recordMaintenanceOperation } from "../../hooks/useMaintenanceHistory";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";

const FILTER_CHIPS = [
  { key: "all",            label: "All" },
  { key: "Bing",           label: "Bing" },
  { key: "Xbox",           label: "Xbox" },
  { key: "Games & promos", label: "Games" },
  { key: "Programs",       label: "Programs" },
  { key: "Windows extras", label: "Windows" },
] as const;

function matchesFilter(item: DebloatItem, filter: string): boolean {
  if (filter === "all") return true;
  if (filter === "Programs") return item.source === "program";
  if (filter === "Windows extras") return item.source === "windows";
  return item.category === filter;
}

export default function DebloatPanel() {
  const requestConfirm = useAppConfirm();
  const { items, loading, errors, bcuInstalled, bcuInstalling, rescan, installBcu } = useDebloatInventory();
  const { createRestorePoint } = useBackend();

  const [selected, setSelected]       = useState<Set<string>>(new Set());
  const [searchQuery, setSearchQuery] = useState("");
  const [activeFilter, setActiveFilter] = useState("all");
  const [restorePoint, setRestorePoint] = useState(true);
  const [removing, setRemoving]         = useState(false);
  const [heroActive, setHeroActive]     = useState(false);

  const filteredItems = useMemo(() => {
    const q = searchQuery.toLowerCase();
    return items.filter(item => {
      const matchSearch = !q || [item.label, item.id, item.publisher, item.version].some(value => value?.toLowerCase().includes(q));
      return matchSearch && matchesFilter(item, activeFilter);
    });
  }, [items, searchQuery, activeFilter]);

  const grouped = useMemo(() => {
    const map = new Map<string, DebloatItem[]>();
    for (const item of filteredItems) {
      const g = map.get(item.category) ?? [];
      g.push(item);
      map.set(item.category, g);
    }
    const ordered = CATEGORY_ORDER.filter(c => map.has(c)).map(c => [c, map.get(c)!] as const);
    const rest = Array.from(map.entries()).filter(([c]) => !CATEGORY_ORDER.includes(c));
    return [...ordered, ...rest];
  }, [filteredItems]);

  const filterCounts = useMemo(() => {
    const c: Record<string, number> = { all: items.length };
    for (const item of items) {
      c[item.category] = (c[item.category] ?? 0) + 1;
    }
    c["Programs"]       = items.filter(i => i.source === "program").length;
    c["Windows extras"] = items.filter(i => i.source === "windows").length;
    return c;
  }, [items]);

  const handleRecommended = useCallback(() => {
    setSelected(new Set(items.filter(i => i.recommended && !i.removalUnavailable).map(i => i.id)));
    setHeroActive(true);
  }, [items]);

  const toggleItem = useCallback((id: string) => {
    setSelected(prev => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
    setHeroActive(false);
  }, []);

  const selectGroup = useCallback((category: string) => {
    const ids = filteredItems.filter(i => i.category === category && !i.removalUnavailable).map(i => i.id);
    setSelected(prev => new Set([...prev, ...ids]));
  }, [filteredItems]);

  const clearSelection = useCallback(() => {
    setSelected(new Set());
    setHeroActive(false);
  }, []);

  const handleRemove = useCallback(async () => {
    const targets = items.filter(i => selected.has(i.id) && !i.removalUnavailable);
    if (!targets.length) return;
    const preview = targets.slice(0, 5).map((item) => item.label).join(", ");
    const remainder = targets.length > 5 ? ` and ${targets.length - 5} more` : "";
    const accepted = await requestConfirm({
      title: `Remove ${targets.length} selected item${targets.length === 1 ? "" : "s"}?`,
      description: `${preview}${remainder}\n\n${restorePoint ? "WinCommander will attempt to create a restore point first." : "Restore point protection is disabled, reducing rollback options."}`,
      confirmLabel: "Remove selected apps",
    });
    if (!accepted) return;
    setRemoving(true);
    try {
      if (restorePoint) {
        try { await createRestorePoint(); }
        catch {
          const accepted = await requestConfirm({
            title: "Continue without a restore point?",
            description: "Windows could not create the requested restore point. Removing the selected apps without it reduces your rollback options.",
            confirmLabel: "Continue removal",
          });
          if (!accepted) {
            setRemoving(false);
            return;
          }
        }
      }
      const succeeded = new Set<string>();
      const failed = new Set<string>();
      const result = await runOperation(
        `Remove ${targets.length} item${targets.length === 1 ? "" : "s"}`,
        targets.map(item => ({
          label: item.label,
          fn: async () => {
            const r = await item.remove();
            if (!r.success) {
              failed.add(item.id);
              throw new Error(r.error ?? "Removal failed");
            }
            succeeded.add(item.id);
          },
        })),
        { mode: "sequential", accent: "neutral", failFast: false }
      );
      recordMaintenanceOperation({ tool: "applications", title: "Remove selected applications", outcome: result.anyError ? (succeeded.size ? "partial" : "failed") : "completed", affectedItems: succeeded.size });
      setSelected(result.anyError ? failed : new Set());
      setHeroActive(false);
      if (result.anyError) {
        showError(`Removed ${succeeded.size}, failed ${failed.size}. Rescan kept failed items selected.`);
      } else {
        showSuccess(`Removed ${succeeded.size} item${succeeded.size === 1 ? "" : "s"}`);
      }
      rescan();
    } catch {
      // runOperation surfaces row-level failures in the status bar
    } finally {
      setRemoving(false);
    }
  }, [items, selected, restorePoint, createRestorePoint, requestConfirm, rescan]);

  return (
    <div className="debloat-panel">
      {/* Hero */}
      <div className="debloat-hero">
        <Button
          icon="endorsed"
          intent={heroActive ? "none" : "success"}
          text={heroActive ? `${selected.size} selected — review below then Remove` : "Recommended selection"}
          small
          onClick={handleRecommended}
          disabled={removing || loading}
        />
        {heroActive && (
          <Button icon="cross" minimal small onClick={clearSelection} disabled={removing} aria-label="Clear recommended debloat selection" />
        )}
        <span className="debloat-hero-sub">
          {heroActive ? "Scroll down to review, then click Remove" : "Preselects known safe-to-remove items"}
        </span>
      </div>

      {/* Toolbar */}
      <div className="debloat-toolbar">
        <InputGroup
          leftIcon="search"
          placeholder="Search applications or publishers…"
          value={searchQuery}
          onChange={e => setSearchQuery(e.target.value)}
          small
          className="font-mono flex-1"
          style={{ background: "var(--color-bg-secondary)", border: "1px solid var(--color-border)" }}
        />
        <div className="debloat-chips">
          {FILTER_CHIPS.map(chip => (
            <button
              key={chip.key}
              className={`debloat-chip-filter${activeFilter === chip.key ? " is-active" : ""}`}
              onClick={() => setActiveFilter(chip.key)}
            >
              {chip.label}
              {filterCounts[chip.key] != null && filterCounts[chip.key] > 0 && (
                <span className="debloat-chip-count">{filterCounts[chip.key]}</span>
              )}
            </button>
          ))}
        </div>
        <Button
          icon="refresh"
          text="RESCAN"
          minimal
          small
          className="font-mono text-[10px]!"
          onClick={rescan}
          loading={loading}
          disabled={loading || removing}
        />
      </div>

      <ApplicationReviewResults
        groups={grouped} selected={selected} removing={removing} loading={loading}
        bcuInstalled={bcuInstalled} bcuInstalling={bcuInstalling} errors={errors}
        onToggle={toggleItem} onSelectGroup={selectGroup} onInstallBcu={() => void installBcu()}
      />

      {/* Selection bar */}
      {selected.size > 0 && (
        <div className="debloat-selbar">
          <span className="debloat-selbar-count">{selected.size} selected</span>
          <span style={{ flex: 1 }} />
          <Checkbox
            checked={restorePoint}
            onChange={e => setRestorePoint((e.target as HTMLInputElement).checked)}
            label="Create restore point"
            className="font-mono text-[10.5px]"
            style={{ marginBottom: 0 }}
          />
          <Button icon="cross" minimal small onClick={clearSelection} disabled={removing} aria-label="Clear debloat selection" />
          <Button
            icon="trash"
            intent="danger"
            text={`Remove (${selected.size})`}
            small
            className="font-mono text-[10px]!"
            onClick={handleRemove}
            loading={removing}
            disabled={removing}
          />
        </div>
      )}
    </div>
  );
}
