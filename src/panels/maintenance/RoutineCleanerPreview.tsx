import { useEffect, useMemo, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import type { RoutineCleanerCategory, RoutineCleanerItem } from "../../hooks/useBackend";
import { formatBytes, getPopulatedRoutineCleanerCategories, getRecommendedItemIds, ROUTINE_CLEANER_CATEGORIES } from "./routineCleanerHelpers";
import type { useRoutineCleaner } from "./useRoutineCleaner";

type RoutineCleaner = ReturnType<typeof useRoutineCleaner>;

interface RoutineCleanerPreviewProps {
  cleaner: RoutineCleaner;
  onRequestClean: () => void;
}

export function RoutineCleanerPreview({ cleaner, onRequestClean }: RoutineCleanerPreviewProps) {
  const { scan, selectedIds, selectedItems, operation, selectRecommended, toggleItem } = cleaner;
  const categoryGroups = useMemo(() => getPopulatedRoutineCleanerCategories(scan?.items ?? []), [scan?.items]);
  const [activeCategory, setActiveCategory] = useState<RoutineCleanerCategory | undefined>(categoryGroups[0]?.id);

  // Results are re-grouped on every scan; keep the active tab pointed at a
  // category that still has matches instead of rendering an empty pane.
  useEffect(() => {
    if (!categoryGroups.length) return;
    if (!categoryGroups.some((group) => group.id === activeCategory)) setActiveCategory(categoryGroups[0].id);
  }, [categoryGroups, activeCategory]);

  if (!scan) return null;

  // Scoped to this tab's category only: recomputes selection for just these
  // items so switching tabs never touches another category's selection.
  const selectRecommendedInCategory = (items: RoutineCleanerItem[]) => {
    const recommendedIds = new Set(getRecommendedItemIds(items));
    items.forEach((item) => {
      if (recommendedIds.has(item.id) !== selectedIds.has(item.id)) toggleItem(item.id);
    });
  };

  return (
    <div className="flex flex-col gap-3">
      <Card className="maintenance-cache-preview-card">
        <CardHeader className="flex-row items-start gap-3">
          <div>
            <CardTitle>Scan preview</CardTitle>
            <CardDescription>
              {scan.cancelled ? "Scan cancelled — showing the partial preview." : `${scan.items.length} cleanable targets found.`}
            </CardDescription>
          </div>
          <Badge tone="accent" className="ml-auto">{formatBytes(scan.totalBytes)}</Badge>
        </CardHeader>
        <CardContent className="flex flex-wrap items-center gap-2">
          <Badge tone="neutral">{scan.totalFiles.toLocaleString()} files</Badge>
          {scan.skippedTargets > 0 && <Badge tone="warning">{scan.skippedTargets} unavailable</Badge>}
          <Button size="sm" variant="outline" onClick={selectRecommended} disabled={operation !== "idle" || !scan.items.length}>
            Select recommended
          </Button>
          <Button size="sm" variant="primary" onClick={onRequestClean} disabled={operation !== "idle" || !selectedItems.length}>
            Clean {selectedItems.length} selected
          </Button>
        </CardContent>
      </Card>

      {scan.items.length === 0 ? (
        <Card>
          <CardContent className="py-6 text-center text-sm text-[var(--text-dim)]">No cleanable cache data found in the selected categories.</CardContent>
        </Card>
      ) : (
        <Tabs value={activeCategory} onValueChange={(value) => setActiveCategory(value as RoutineCleanerCategory)}>
          <TabsList className="flex-wrap">
            {categoryGroups.map((group) => (
              <TabsTrigger key={group.id} value={group.id} className="gap-1.5">
                {group.label}
                <Badge tone="neutral">{group.items.length}</Badge>
              </TabsTrigger>
            ))}
          </TabsList>
          {categoryGroups.map((group) => (
            <TabsContent key={group.id} value={group.id}>
              <div className="mb-3">
                <Button size="sm" variant="outline" onClick={() => selectRecommendedInCategory(group.items)} disabled={operation !== "idle"}>
                  Select recommended
                </Button>
              </div>
              <div className="flex flex-col gap-2">
                {group.items.map((item) => (
                  <ItemRow key={item.id} item={item} selected={selectedIds.has(item.id)} disabled={operation !== "idle"} onToggle={toggleItem} />
                ))}
              </div>
            </TabsContent>
          ))}
        </Tabs>
      )}
    </div>
  );
}

interface ItemRowProps {
  item: RoutineCleanerItem;
  selected: boolean;
  disabled: boolean;
  onToggle: (id: string) => void;
}

function ItemRow({ item, selected, disabled, onToggle }: ItemRowProps) {
  const category = ROUTINE_CLEANER_CATEGORIES.find((entry) => entry.id === item.category)?.label ?? item.category;
  return (
    <label className="flex cursor-pointer items-start gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-2.5 hover:border-[var(--border-strong)] has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-60">
      <input
        className="mt-0.5 wc-check"
        type="checkbox"
        checked={selected}
        disabled={disabled}
        onChange={() => onToggle(item.id)}
      />
      <span className="min-w-0 flex-1">
        <span className="flex flex-wrap items-center gap-2 text-sm font-medium text-[var(--text)]">
          {item.label}
          {item.recommended && <Badge tone="success">Recommended</Badge>}
          {item.operation === "vacuum" && <Badge tone="warning">Optimize</Badge>}
          {item.truncated && <Badge tone="warning">Large target</Badge>}
        </span>
        <span className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-[var(--text-dim)]">
          <Badge tone="neutral">{category}</Badge>
          <span className="min-w-0 truncate font-mono" title={item.path}>{item.path}</span>
        </span>
      </span>
      <span className="shrink-0 font-mono text-xs text-[var(--text-dim)]">{formatBytes(item.bytes)}</span>
    </label>
  );
}
