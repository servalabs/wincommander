import { Fragment, useMemo, useState } from "react";
import { appIcons } from "../../assets/apps";
import { browserLogos } from "../../assets/privacy";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { CheckboxControl } from "../../components/ui/bp";
import { Icon, type IconName } from "../../components/ui/icon";
import type { RoutineCleanerCategory, RoutineCleanerItem } from "../../hooks/useBackend";
import { resolveBrowserIconUrl } from "../privacy/browserIcons";
import { toExplorerDisplayPath } from "./maintenancePathDisplay";
import { formatBytes, getRoutineCleanerCategories } from "./routineCleanerHelpers";
import type { useRoutineCleaner } from "./useRoutineCleaner";

type RoutineCleaner = ReturnType<typeof useRoutineCleaner>;
const CATEGORY_ICONS: Partial<Record<RoutineCleanerCategory, IconName>> = {
  system: "hard-drive",
  browsers: "globe",
  applications: "applications",
  developers: "code",
  projects: "folder-close",
  ai: "lightbulb",
  gaming: "application",
  databases: "database",
};

const BROWSER_LABELS = [
  "Opera GX", "LibreWolf", "Floorp", "Vivaldi", "Firefox", "Brave", "Opera", "Chrome", "Edge",
];
const CLEANUP_APP_ICON_FILES: Record<string, string> = {
  "Discord": "discord.svg",
  "Microsoft Teams": "teams.svg",
  "Slack": "slack.svg",
  "Zoom": "zoom.svg",
  "Telegram": "telegram.svg",
  "VS Code": "vscode.svg",
  "JetBrains IDEs": "intellij.svg",
  "Spotify": "spotify.svg",
  "OBS Studio": "obs.svg",
  "Bun Cache": "bun.svg",
  "Docker Desktop": "docker.svg",
  "Cursor IDE": "cursor.png",
  "Signal Desktop": "signal.svg",
  "GitHub Desktop": "github-desktop.svg",
  "VLC": "vlc.svg",
  "Thunderbird": "thunderbird.svg",
  "Obsidian": "obsidian.svg",
  "WhatsApp Desktop": "whatsapp.svg",
  "Claude": "claude.svg",
  "Steam Launcher": "steam.svg",
  "Epic Games Launcher": "epic-games.svg",
};

export function RoutineCleanerPreview({ cleaner }: { cleaner: RoutineCleaner }) {
  const { scan, selectedIds, operation } = cleaner;
  const [activeCategory, setActiveCategory] = useState<string>();
  const [focusedId, setFocusedId] = useState<string>();
  const [query, setQuery] = useState("");
  const groups = useMemo(() => getRoutineCleanerCategories([...new Set([...cleaner.categories, ...(scan?.items.map((item) => item.category) ?? [])])]).map((category) => ({ ...category, items: scan?.items.filter((item) => item.category === category.id) ?? [] })), [cleaner.categories, scan?.items]);
  const visibleCategory = groups.find((group) => group.id === activeCategory) ?? groups.find((group) => group.items.length) ?? groups[0];
  const visibleItems = (visibleCategory?.items ?? []).filter((item) => `${item.label} ${item.pathDisplay ?? ""}`.toLowerCase().includes(query.toLowerCase()));
  const focusedItem = visibleItems.find((item) => item.id === focusedId);
  const scanSizes = summarizeItems(scan?.items ?? []);
  const isBusy = operation !== "idle";
  const isCategorySelected = !!visibleCategory?.items.length && visibleCategory.items.every((item) => selectedIds.has(item.id));
  if (!scan) return null;

  return <div className="maintenance-cache-workspace">
    <div className="maintenance-findings-summary" role="status" aria-live="polite">
      <span><strong>{scan.items.length}</strong> targets · <strong>{scan.totalFiles.toLocaleString()}</strong> file records in the scan</span>
      <span>Logical file size <strong className="text-[var(--accent)]">{formatBytes(scanSizes.logicalBytes)}</strong></span>
      {scanSizes.vacuumEstimateBytes > 0 && <span>Database optimization estimate <strong className="text-[var(--accent)]">{formatBytes(scanSizes.vacuumEstimateBytes)}</strong></span>}
      {scan.cancelled && <Badge tone="warning">Partial scan · cancelled</Badge>}
      {scan.skippedTargets > 0 && <Badge tone="warning">{scan.skippedTargets} unavailable targets</Badge>}
      {scan.items.some((item) => item.truncated) && <Badge tone="warning">Some target scans were partial</Badge>}
    </div>
    <div className="maintenance-cache-master-detail">
      <nav className="maintenance-cache-categories" aria-label="Cache categories">
        {groups.map((group) => {
          const sizes = summarizeItems(group.items);
          return <button key={group.id} type="button" onClick={() => { setActiveCategory(group.id); setFocusedId(undefined); setQuery(""); }} aria-current={group.id === visibleCategory?.id ? "true" : undefined}>
            <Icon icon={CATEGORY_ICONS[group.id] ?? "applications"} size={16} />
            <span><strong>{group.label}</strong><small>{group.items.length} targets · {formatBytes(sizes.logicalBytes)} logical{sizes.vacuumEstimateBytes ? ` · ${formatBytes(sizes.vacuumEstimateBytes)} estimated` : ""}</small></span>
            {group.items.some((item) => selectedIds.has(item.id)) && <span className="maintenance-category-selected" aria-label="Contains selected targets" />}
          </button>;
        })}
      </nav>
      <section className="maintenance-cache-results" aria-label={`${visibleCategory?.label ?? "Cache"} targets`}>
        <div className="maintenance-cache-results-header">
          <div><strong>{visibleCategory?.label}</strong><p>{visibleCategory?.description}</p></div>
          <span>{formatBytes(summarizeItems((visibleCategory?.items ?? []).filter((item) => selectedIds.has(item.id))).logicalBytes)} logical selected</span>
          <Button size="sm" variant="ghost" aria-pressed={isCategorySelected} aria-label={`${isCategorySelected ? "Deselect" : "Select"} all ${visibleCategory?.label ?? "cache"} targets`} disabled={isBusy || !visibleCategory?.items.length} onClick={() => { if (visibleCategory) cleaner.toggleCategorySelection(visibleCategory.id); }}>{isCategorySelected ? "Deselect category" : "Select category"}</Button>
        </div>
        <label className="maintenance-cache-search"><Icon icon="search" size={14} /><input aria-label="Filter cache targets" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Filter by name or location" /></label>
        <div className="maintenance-cache-targets">
          {visibleItems.map((item) => <Fragment key={item.id}><ItemRow item={item} selected={selectedIds.has(item.id)} focused={focusedItem?.id === item.id} disabled={isBusy} onToggle={cleaner.toggleItem} onDetails={() => setFocusedId(focusedId === item.id ? undefined : item.id)} />{focusedItem?.id === item.id && <CacheItemDetail item={item} />}</Fragment>)}
          {!visibleItems.length && <div className="maintenance-cache-empty"><Icon icon="confirm" size={28} /><strong>{query ? "No matching targets" : "No cleanable data found"}</strong><p>{query ? "Try another name or location." : "This category has no cache targets in the current scan."}</p></div>}
        </div>
      </section>
    </div>
  </div>;
}

function ItemRow({ item, selected, focused, disabled, onToggle, onDetails }: { item: RoutineCleanerItem; selected: boolean; focused: boolean; disabled: boolean; onToggle: (id: string) => void; onDetails: () => void }) {
  const categoryIcon = CATEGORY_ICONS[item.category] ?? "applications";
  const logo = browserLogoForLabel(item.label) ?? appLogoForLabel(item.label);
  const amountLabel = item.operation === "vacuum" ? "estimated reclaim" : "logical size";
  return <div className="maintenance-cache-target" data-selected={selected || undefined}>
    <CheckboxControl checked={selected} disabled={disabled} ariaLabel={`${selected ? "Deselect" : "Select"} ${item.label}`} onChange={() => onToggle(item.id)} />
    {logo ? <img src={logo} alt="" width={16} height={16} className="shrink-0 object-contain" /> : <Icon icon={item.operation === "vacuum" ? "database" : categoryIcon} size={16} className="text-[var(--text-mute)]" />}
    <button className="maintenance-cache-target-description" type="button" onClick={onDetails} aria-expanded={focused}>
      <span><strong>{item.label}</strong>{item.recommended && <Badge tone="success">Recommended</Badge>}{item.truncated && <Badge tone="warning">Partial scan</Badge>}</span>
      <small>{item.operation === "vacuum" ? "SQLite database · contents retained" : "Regenerable cache files · review folder and entries"}</small>
    </button>
    <span className="maintenance-cache-target-size">{formatBytes(item.bytes)}<small>{amountLabel} · {item.fileCount.toLocaleString()} files counted</small></span>
    <Button size="icon" variant="ghost" aria-label={`Details for ${item.label}`} aria-expanded={focused} onClick={onDetails}><Icon icon={focused ? "chevron-up" : "chevron-down"} size={14} /></Button>
  </div>;
}

function CacheItemDetail({ item }: { item: RoutineCleanerItem }) {
  const entries = item.entries ?? [];
  const entryCount = item.entryCount ?? item.fileCount;
  const displayLimit = item.entriesLimit ?? entries.length;
  const pathDisplay = toExplorerDisplayPath(item.pathDisplay);
  const entryScanIsPartial = item.truncated;
  return <aside className="maintenance-cache-detail" aria-label={`${item.label} details`}>
    <div className="flex flex-wrap items-start justify-between gap-2">
      <div><strong>{item.label}</strong><p className="mt-1 text-[var(--text-mute)]">{item.description || (item.operation === "vacuum" ? "SQLite database optimization target." : "Scanned folder of cleanable cache data.")}</p></div>
      <span className="text-xs text-[var(--text-dim)]">{entryCount.toLocaleString()} file{entryCount === 1 ? "" : "s"} in this scan</span>
    </div>
    <div className="mt-3 rounded border border-[var(--border)] bg-[var(--surface)] p-2">
      <span className="block text-[10px] uppercase tracking-wide text-[var(--text-mute)]">Exact target folder</span>
      <span className="mt-1 block whitespace-pre-wrap font-mono text-[11px] [overflow-wrap:anywhere]">{pathDisplay ?? "Path unavailable: it could not be decoded safely."}</span>
    </div>
    <p>{item.operation === "vacuum" ? "The database remains in place. This is a potential reclaim estimate; the actual size change is reported only after optimization finishes." : "These are measured logical file sizes. Removing them may let apps rebuild or download the cache again; actual free-space change can differ."}</p>
    <p>{item.recommended ? "Included in the recommended selection." : "Optional target. Review this folder and its file list before selecting it."} The backend uses the scanned target ID and revalidates files before cleaning.</p>
    {entryScanIsPartial && <p className="text-[var(--warn)]">This target scan was partial: inaccessible or link-like entries may have been skipped, or a scan safety limit was reached. The listed count and amount may be incomplete.</p>}
    {item.entriesTruncated && <p className="text-[var(--warn)]">Showing the first {entries.length.toLocaleString()} of {entryCount.toLocaleString()} file entries from this scan (display limit {displayLimit.toLocaleString()}).</p>}
    <div className="mt-3">
      <strong className="text-xs">Files included in this preview</strong>
      {entries.length ? <ul className="mt-2 max-h-64 overflow-auto rounded border border-[var(--border)] bg-[var(--surface)]" aria-label={`${item.label} scanned files`}>
        {entries.map((entry) => {
          const filePath = toExplorerDisplayPath(entry.path);
          return <li key={entry.id} className="flex flex-wrap items-start justify-between gap-2 border-b border-[var(--border)] px-2 py-2 last:border-0">
            <span className="min-w-0 flex-1 whitespace-pre-wrap font-mono text-[10px] text-[var(--text-dim)] [overflow-wrap:anywhere]">{filePath ?? "Path unavailable: it could not be decoded safely."}</span>
            <span className="shrink-0 text-right text-[10px] text-[var(--text-mute)]">{formatBytes(entry.actualSizeBytes)} file size{entry.estimatedReclaimBytes !== null && <span className="block">{formatBytes(entry.estimatedReclaimBytes)} estimated reclaim</span>}</span>
          </li>;
        })}
      </ul> : <p className="mt-1 text-xs text-[var(--text-mute)]">The backend did not return file-level details for this target.</p>}
    </div>
  </aside>;
}

function summarizeItems(items: RoutineCleanerItem[]) {
  return items.reduce((summary, item) => {
    if (item.operation === "vacuum") summary.vacuumEstimateBytes += item.bytes;
    else summary.logicalBytes += item.bytes;
    return summary;
  }, { logicalBytes: 0, vacuumEstimateBytes: 0 });
}

function browserLogoForLabel(label: string): string | undefined {
  const browserLabel = BROWSER_LABELS.find((name) => label === name || label.startsWith(`${name} `) || label.startsWith(`${name} - `));
  if (!browserLabel) return undefined;
  const iconName = browserLabel === "Chrome" ? "Google Chrome" : browserLabel === "Edge" ? "Microsoft Edge" : browserLabel;
  return resolveBrowserIconUrl(iconName, browserLogos);
}

function appLogoForLabel(label: string): string | undefined {
  const filename = CLEANUP_APP_ICON_FILES[label];
  return filename ? appIcons[filename] : undefined;
}
