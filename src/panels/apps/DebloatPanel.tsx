// src/panels/apps/DebloatPanel.tsx
import { Button, Checkbox, Icon, InputGroup, Spinner } from "@/components/ui/bp";
import { useCallback, useMemo, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { runOperation } from "../../context/OperationContext";
import useBackend from "../../hooks/useBackend";
import { showError, showSuccess } from "../../utils/toast";
import AppIcon from "./components/AppIcon";
import { CATEGORY_ORDER } from "./debloatLists";
import { DebloatItem } from "./types";
import { useDebloatInventory } from "./useDebloatInventory";
// staggerDelay caps per-item delay — large groups won't animate for seconds.
import { staggerDelay } from "../../components/shared/AnimatedList";
import { DURATION_S, EASE } from "../../components/shared/motion";
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

function SourceBadge({ source }: { source: DebloatItem["source"] }) {
  const { label, color, border } = source === "store"
    ? { label: "ST",  color: "#5bd6c7", border: "rgba(55,182,166,.5)" }
    : source === "program"
    ? { label: "PR",  color: "#a78bff", border: "rgba(139,92,246,.5)" }
    : { label: "WIN", color: "#f0a93b", border: "rgba(214,164,24,.5)" };
  return (
    <span style={{ color, borderColor: border, fontFamily: "var(--font-mono)", fontSize: "8px", padding: "1px 4px", borderRadius: "3px", border: "1px solid", flexShrink: 0 }}>
      {label}
    </span>
  );
}

function DebloatChip({ item, selected, onToggle, removing }: {
  item: DebloatItem; selected: boolean; onToggle: (id: string) => void; removing: boolean;
}) {
  return (
    <button
      type="button"
      className={`debloat-chip${selected ? " is-selected" : ""}`}
      onClick={() => onToggle(item.id)}
      aria-pressed={selected}
      aria-label={`${selected ? "Deselect" : "Select"} ${item.label}`}
      disabled={removing}
    >
      <span className="debloat-chip-check" aria-hidden="true">
        {selected && <Icon icon="tick" size={10} />}
      </span>
      {/* Real brand icon alongside the source badge — SourceBadge alone was
          the only visual identity (a generic 2-3 letter text chip). Reuses
          the same resolver the app catalog uses; falls back to a category
          glyph when no bundled asset matches. */}
      <AppIcon id={item.id} category={item.category} iconData={item.iconData} size={22} preferNative />
      <span className="debloat-chip-name" title={item.id}>{item.label}</span>
      {item.riskNote && (
        <span title={item.riskNote} style={{ flexShrink: 0, cursor: "help" }}>
          <Icon icon="warning-sign" size={10} className="text-[var(--color-warning)]" />
        </span>
      )}
      <SourceBadge source={item.source} />
    </button>
  );
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
      const matchSearch = !q || item.label.toLowerCase().includes(q) || item.id.toLowerCase().includes(q);
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
    setSelected(new Set(items.filter(i => i.recommended).map(i => i.id)));
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
    const ids = items.filter(i => i.category === category).map(i => i.id);
    setSelected(prev => new Set([...prev, ...ids]));
  }, [items]);

  const clearSelection = useCallback(() => {
    setSelected(new Set());
    setHeroActive(false);
  }, []);

  const handleRemove = useCallback(async () => {
    const targets = items.filter(i => selected.has(i.id));
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
          text={heroActive ? `${selected.size} selected — review below then Remove` : "⚡ Recommended Debloat"}
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
          placeholder="Filter items…"
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

      {/* Grid */}
      <div className="debloat-grid-scroll">
        {loading && items.length === 0 ? (
          <div className="flex items-center justify-center py-8 gap-2">
            <Spinner size={18} />
            <span className="text-xs text-[var(--color-text-muted)] font-mono">Scanning…</span>
          </div>
        ) : (
          <>
            {errors.store && (
              <div className="debloat-group-error">
                <Icon icon="warning-sign" size={12} />
                Store apps: {errors.store}
              </div>
            )}
            {grouped.map(([category, groupItems]) => (
              <div key={category} className="debloat-group">
                <div className="debloat-cath">
                  <span>{category.toUpperCase()}</span>
                  {category === "Programs" && !bcuInstalled ? (
                    <button
                      className="debloat-install-prompt"
                      onClick={installBcu}
                      disabled={bcuInstalling}
                    >
                      {bcuInstalling ? "Installing…" : "Install engine to scan →"}
                    </button>
                  ) : (
                    <button className="debloat-select-all" onClick={() => selectGroup(category)}>
                      select all
                    </button>
                  )}
                </div>
                {errors.programs && category === "Programs" && (
                  <div className="debloat-group-error text-xs">{errors.programs}</div>
                )}
                {/* AnimatePresence lets removed chips collapse instead of snapping
                    away. motion.div uses grid-template-rows trick (no reflow) for
                    height; opacity on the outer handles the cross-fade.
                    No success flourish on removal — enter/exit only per DN-07. */}
                <div className="debloat-grid">
                  <AnimatePresence initial={false}>
                    {groupItems.map((item, idx) => (
                      // Outer: opacity fade (compositor-safe, no reflow).
                      // Inner: grid-template-rows 0fr→1fr for height collapse
                      // without triggering layout on every animation frame.
                      <motion.div
                        key={item.id}
                        initial={{ opacity: 0, gridTemplateRows: "0fr" }}
                        animate={{
                          opacity: 1,
                          gridTemplateRows: "1fr",
                          transition: {
                            opacity: { delay: staggerDelay(idx), duration: DURATION_S.normal, ease: EASE.enter },
                            gridTemplateRows: { delay: staggerDelay(idx), duration: DURATION_S.normal, ease: EASE.enter },
                          },
                        }}
                        exit={{
                          opacity: 0,
                          gridTemplateRows: "0fr",
                          transition: {
                            opacity: { duration: DURATION_S.fast, ease: EASE.exit },
                            gridTemplateRows: { duration: DURATION_S.fast, ease: EASE.exit },
                          },
                        }}
                        style={{ display: "grid", overflow: "hidden" }}
                      >
                        {/* min-height:0 child required by the grid-template-rows trick. */}
                        <div style={{ minHeight: 0 }}>
                          <DebloatChip
                            item={item}
                            selected={selected.has(item.id)}
                            onToggle={toggleItem}
                            removing={removing}
                          />
                        </div>
                      </motion.div>
                    ))}
                  </AnimatePresence>
                </div>
              </div>
            ))}
            {!loading && grouped.length === 0 && (
              <div className="text-center text-xs text-[var(--color-text-muted)] py-6">
                {searchQuery ? "No items match your search." : "Nothing to remove — system is clean."}
              </div>
            )}
          </>
        )}
      </div>

      {/* Selection bar */}
      {selected.size > 0 && (
        <div className="debloat-selbar">
          <span className="debloat-selbar-count">▣ {selected.size} selected</span>
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
