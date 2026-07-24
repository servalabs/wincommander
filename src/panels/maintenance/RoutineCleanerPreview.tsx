import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import type { RoutineCleanerCategory } from "../../hooks/useBackend";
import { formatBytes, groupRoutineCleanerItems, ROUTINE_CLEANER_CATEGORIES } from "./routineCleanerHelpers";
import type { useRoutineCleaner } from "./useRoutineCleaner";

type RoutineCleaner = ReturnType<typeof useRoutineCleaner>;

interface RoutineCleanerPreviewProps {
  cleaner: RoutineCleaner;
  onRequestClean: () => void;
}

export function RoutineCleanerPreview({ cleaner, onRequestClean }: RoutineCleanerPreviewProps) {
  const { scan, selectedIds, selectedItems, operation, selectRecommended, toggleItem } = cleaner;
  if (!scan) return null;
  const groups = groupRoutineCleanerItems(scan.items);

  return (
    <div className="flex flex-col gap-3">
      <Card>
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
      ) : ROUTINE_CLEANER_CATEGORIES.map((category) => (
        <ItemGroup
          key={category.id}
          label={category.label}
          items={groups[category.id] ?? []}
          selectedIds={selectedIds}
          disabled={operation !== "idle"}
          onToggle={toggleItem}
        />
      ))}
    </div>
  );
}

interface ItemGroupProps {
  label: string;
  items: ReturnType<typeof groupRoutineCleanerItems>[RoutineCleanerCategory];
  selectedIds: Set<string>;
  disabled: boolean;
  onToggle: (id: string) => void;
}

function ItemGroup({ label, items = [], selectedIds, disabled, onToggle }: ItemGroupProps) {
  if (!items.length) return null;
  return (
    <Card>
      <CardHeader className="flex-row items-center gap-3">
        <CardTitle>{label}</CardTitle>
        <Badge tone="neutral">{items.length}</Badge>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {items.map((item) => (
          <label key={item.id} className="flex cursor-pointer items-start gap-3 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 py-2.5 hover:border-[var(--border-strong)] has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-60">
            <input
              className="mt-0.5 size-4 accent-[var(--accent)]"
              type="checkbox"
              checked={selectedIds.has(item.id)}
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
              <span className="mt-1 block truncate font-mono text-[11px] text-[var(--text-dim)]" title={item.path}>{item.path}</span>
            </span>
            <span className="shrink-0 font-mono text-xs text-[var(--text-dim)]">{formatBytes(item.bytes)}</span>
          </label>
        ))}
      </CardContent>
    </Card>
  );
}
