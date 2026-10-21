import { Fragment, useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { CheckboxControl } from "../../components/ui/bp";
import { Icon, type IconName } from "../../components/ui/icon";
import type { RoutineCleanerCategory, RoutineCleanerItem } from "../../hooks/useBackend";
import { formatBytes, getRoutineCleanerCategories } from "./routineCleanerHelpers";
import type { useRoutineCleaner } from "./useRoutineCleaner";

type RoutineCleaner = ReturnType<typeof useRoutineCleaner>;
const CATEGORY_ICONS: Partial<Record<RoutineCleanerCategory, IconName>> = { system: "hard-drive", browsers: "globe", applications: "applications", developers: "code", projects: "folder-close", ai: "lightbulb", gaming: "application", databases: "database" };

export function RoutineCleanerPreview({ cleaner, onRequestClean }: { cleaner: RoutineCleaner; onRequestClean: () => void }) {
  const { scan, selectedIds, selectedItems, operation } = cleaner;
  const [activeCategory, setActiveCategory] = useState<string>();
  const [focusedId, setFocusedId] = useState<string>();
  const [query, setQuery] = useState("");
  const groups = useMemo(() => getRoutineCleanerCategories(cleaner.scanScope === "projects" ? ["projects"] : [...new Set([...cleaner.categories, ...(scan?.items.map((item) => item.category) ?? [])])]).map((category) => ({ ...category, items: scan?.items.filter((item) => item.category === category.id) ?? [] })), [cleaner.categories, cleaner.scanScope, scan?.items]);
  const visibleCategory = groups.find((group) => group.id === activeCategory) ?? groups.find((group) => group.items.length) ?? groups[0];
  const visibleItems = (visibleCategory?.items ?? []).filter((item) => `${item.label} ${item.path}`.toLowerCase().includes(query.toLowerCase()));
  const focusedItem = visibleItems.find((item) => item.id === focusedId);
  const selectedBytes = selectedItems.reduce((sum, item) => sum + item.bytes, 0);
  const isBusy = operation !== "idle";
  const isCategorySelected = !!visibleCategory?.items.length && visibleCategory.items.every((item) => selectedIds.has(item.id));
  if (!scan) return null;

  return <div className="maintenance-cache-workspace">
    <div className="maintenance-findings-summary" role="status" aria-live="polite">
      {cleaner.scanScope === "projects" && <Badge tone="neutral">Chosen project folder</Badge>}
      {cleaner.scanScope === "projects" && scan.scanRoot && <span className="break-all text-xs text-[var(--text-dim)]" title={scan.scanRoot}>{scan.scanRoot}</span>}
      <span><strong>{scan.items.length}</strong> targets found <span className="text-[var(--text-mute)]">· {scan.totalFiles.toLocaleString()} files</span></span>
      <span>Estimated logical data <strong className="text-[var(--accent)]">{formatBytes(scan.totalBytes)}</strong></span>
      {scan.cancelled && <Badge tone="warning">Partial scan · cancelled</Badge>}
      {scan.skippedTargets > 0 && <Badge tone="warning">{scan.skippedTargets} unavailable targets</Badge>}
      {scan.items.some((item) => item.truncated) && <Badge tone="warning">Some targets only partially measured</Badge>}
    </div>
    <div className="maintenance-cache-master-detail">
      <nav className="maintenance-cache-categories" aria-label="Cache categories">
        {groups.map((group) => <button key={group.id} type="button" onClick={() => { setActiveCategory(group.id); setFocusedId(undefined); setQuery(""); }} aria-current={group.id === visibleCategory?.id ? "true" : undefined}>
          <Icon icon={CATEGORY_ICONS[group.id] ?? "applications"} size={16} />
          <span><strong>{group.label}</strong><small>{formatBytes(group.items.reduce((sum, item) => sum + item.bytes, 0))} · {group.items.length} targets</small></span>
          {group.items.some((item) => selectedIds.has(item.id)) && <span className="maintenance-category-selected" aria-label="Contains selected targets" />}
        </button>)}
      </nav>
      <section className="maintenance-cache-results" aria-label={`${visibleCategory?.label ?? "Cache"} targets`}>
        <div className="maintenance-cache-results-header"><div><strong>{visibleCategory?.label}</strong><p>{visibleCategory?.description}</p></div><span>{formatBytes((visibleCategory?.items ?? []).filter((item) => selectedIds.has(item.id)).reduce((sum, item) => sum + item.bytes, 0))} selected</span><Button size="sm" variant="ghost" aria-pressed={isCategorySelected} aria-label={`${isCategorySelected ? "Deselect" : "Select"} all ${visibleCategory?.label ?? "cache"} targets`} disabled={isBusy || !visibleCategory?.items.length} onClick={() => { if (visibleCategory) cleaner.toggleCategorySelection(visibleCategory.id); }}>{isCategorySelected ? "Deselect category" : "Select category"}</Button></div>
        <label className="maintenance-cache-search"><Icon icon="search" size={14} /><input aria-label="Filter cache targets" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Filter by name or location" /></label>
        <div className="maintenance-cache-targets">
          {visibleItems.map((item) => <Fragment key={item.id}><ItemRow item={item} selected={selectedIds.has(item.id)} focused={focusedItem?.id === item.id} disabled={isBusy} onToggle={cleaner.toggleItem} onDetails={() => setFocusedId(focusedId === item.id ? undefined : item.id)} />{focusedItem?.id === item.id && <CacheItemDetail item={item} />}</Fragment>)}
          {!visibleItems.length && <div className="maintenance-cache-empty"><Icon icon="confirm" size={28} /><strong>{query ? "No matching targets" : "No cleanable data found"}</strong><p>{query ? "Try another name or location." : "This category has no cache targets in the current scan."}</p></div>}
        </div>
      </section>
    </div>
    <footer className="maintenance-selection-bar">
      <div><strong>{formatBytes(selectedBytes)}</strong><span>{selectedItems.length} targets selected · estimated logical data</span></div>
      <div className="flex flex-wrap gap-2"><Button size="sm" variant="ghost" onClick={cleaner.clearSelection} disabled={isBusy || !selectedItems.length}>Clear</Button><Button size="sm" variant="outline" onClick={cleaner.selectRecommended} disabled={isBusy || !scan.items.length}>Select recommended</Button><Button size="sm" variant="primary" onClick={onRequestClean} disabled={isBusy || !selectedItems.length}><Icon icon="clean" /> Clean selected</Button></div>
    </footer>
  </div>;
}

function ItemRow({ item, selected, focused, disabled, onToggle, onDetails }: { item: RoutineCleanerItem; selected: boolean; focused: boolean; disabled: boolean; onToggle: (id: string) => void; onDetails: () => void }) {
  return <div className="maintenance-cache-target" data-selected={selected || undefined}>
    <CheckboxControl checked={selected} disabled={disabled} ariaLabel={`${selected ? "Deselect" : "Select"} ${item.label}`} onChange={() => onToggle(item.id)} />
    <Icon icon={item.operation === "vacuum" ? "database" : "application"} size={16} className="text-[var(--text-mute)]" />
    <button className="maintenance-cache-target-description" type="button" onClick={onDetails} aria-expanded={focused}>
      <span><strong>{item.label}</strong>{item.recommended && <Badge tone="success">Recommended</Badge>}{item.truncated && <Badge tone="warning">Partial estimate</Badge>}</span>
      <small>{item.operation === "vacuum" ? "Optimize SQLite database; data is retained" : "Regenerable cache · review location and impact"}</small>
    </button>
    <span className="maintenance-cache-target-size">{formatBytes(item.bytes)}<small>{item.fileCount.toLocaleString()} files</small></span>
    <Button size="icon" variant="ghost" aria-label={`Details for ${item.label}`} onClick={onDetails}><Icon icon={focused ? "chevron-up" : "chevron-down"} size={14} /></Button>
  </div>;
}

function CacheItemDetail({ item }: { item: RoutineCleanerItem }) {
  return <aside className="maintenance-cache-detail" aria-label={`${item.label} details`}>
    <strong>{item.label}</strong><p className="break-all font-mono">{item.path}</p>
    <p>{item.operation === "vacuum" ? "SQLite optimization can remove unused database pages. The logical size estimate may differ from the amount removed." : "Cleaning permanently removes the scanned cache data. Applications may download or rebuild it the next time they run; close the owning application before cleaning."}</p>
    <p>{item.recommended ? "Included in the recommended selection." : "Optional target. Review this location before selecting it."} {item.truncated ? "The scan limit was reached, so this target's size is an incomplete estimate." : "The backend revalidates the target before modifying it."}</p>
  </aside>;
}
