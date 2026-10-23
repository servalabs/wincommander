import { useMemo, useRef, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { CheckboxControl } from "../../components/ui/bp";
import { Icon, type IconName } from "../../components/ui/icon";
import type { DuplicateGroup } from "../../hooks/useBackend";
import { formatBytes } from "./routineCleanerHelpers";
import type { useFileHygiene } from "./useFileHygiene";
import { toExplorerDisplayPath } from "./maintenancePathDisplay";
import { hasExpandedDuplicateGroups, isDuplicateGroupExpanded, setDuplicateGroupExpanded, toggleDuplicateGroups } from "./duplicateGroupPresentation";
import "./DuplicateGroups.css";

type Tools = ReturnType<typeof useFileHygiene>;
const TYPES: { id: string; label: string; icon: IconName; extensions: string[] }[] = [
  { id: "images", label: "Images", icon: "media", extensions: ["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "heic"] },
  { id: "video", label: "Video", icon: "video", extensions: ["mp4", "mkv", "mov", "avi", "wmv", "webm"] },
  { id: "audio", label: "Audio", icon: "music", extensions: ["mp3", "wav", "flac", "m4a", "ogg", "aac"] },
  { id: "documents", label: "Documents", icon: "document", extensions: ["pdf", "doc", "docx", "txt", "md", "rtf", "xls", "xlsx", "csv"] },
  { id: "archives", label: "Archives", icon: "compressed", extensions: ["zip", "rar", "7z", "tar", "gz", "iso"] },
];
const fileType = (name: string) => TYPES.find((type) => type.extensions.includes(name.split(".").pop()?.toLowerCase() ?? ""));

function formatModified(modified: string | null) {
  if (!modified || !/^\d+$/.test(modified)) return "Modified date unavailable";
  try {
    const milliseconds = Number(BigInt(modified) / 1_000_000n);
    if (!Number.isFinite(milliseconds)) return "Modified date unavailable";
    return `Modified ${new Date(milliseconds).toLocaleString()}`;
  } catch { return "Modified date unavailable"; }
}

export function DuplicateGroups({ tools }: { tools: Tools }) {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("all");
  const [visibleCount, setVisibleCount] = useState(50);
  const [expandedGroupIds, setExpandedGroupIds] = useState<Set<string> | null>(null);
  const firstMatchRef = useRef<HTMLDetailsElement>(null);
  const groups = tools.duplicateScan?.groups;
  const filtered = useMemo(() => (groups ?? []).filter((group) =>
    (category === "all" || group.files.some((file) => (fileType(file.name)?.id ?? "other") === category)) &&
    group.files.some((file) => `${file.name} ${toExplorerDisplayPath(file.path) ?? ""}`.toLowerCase().includes(query.trim().toLowerCase())),
  ), [category, groups, query]);
  const groupKey = (group: DuplicateGroup) => `${group.id}:${group.memberOffset}`;
  const isExpanded = (group: DuplicateGroup) => isDuplicateGroupExpanded(expandedGroupIds, groupKey(group));
  const setGroupExpanded = (group: DuplicateGroup, open: boolean) => {
    setExpandedGroupIds((current) => setDuplicateGroupExpanded(current, (groups ?? []).map(groupKey), groupKey(group), open));
  };
  const hasExpandedGroups = hasExpandedDuplicateGroups(expandedGroupIds);
  const toggleAll = () => setExpandedGroupIds((current) => toggleDuplicateGroups(current));
  const onSearch = (event: React.FormEvent) => {
    event.preventDefault();
    setVisibleCount(50);
    if (filtered[0]) setExpandedGroupIds((current) => {
      if (current === null) return current;
      return setDuplicateGroupExpanded(current, (groups ?? []).map(groupKey), groupKey(filtered[0]), true);
    });
    requestAnimationFrame(() => firstMatchRef.current?.querySelector("summary")?.focus());
  };
  return <section className="duplicate-workspace" aria-label="Discovered duplicate groups">
    <form className="duplicate-filters" onSubmit={onSearch}>
      <label><Icon icon="search" size={14} /><input aria-label="Search duplicate files and locations" value={query} onChange={(event) => { setQuery(event.target.value); setVisibleCount(50); }} placeholder="Search file names or locations" /></label>
      <select aria-label="Duplicate file type" value={category} onChange={(event) => { setCategory(event.target.value); setVisibleCount(50); }}>
        <option value="all">All file types</option>{TYPES.map((type) => <option key={type.id} value={type.id}>{type.label}</option>)}<option value="other">Other</option>
      </select>
      <span>{filtered.length} of {groups?.length ?? 0} loaded groups · filters apply to this page</span>
      <Button size="sm" variant="outline" type="button" disabled={!filtered.length} onClick={toggleAll}>{hasExpandedGroups ? "Collapse all" : "Expand all"}</Button>
    </form>
    {filtered.slice(0, visibleCount).map((group, index) => <DuplicateGroupRows key={groupKey(group)} group={group} tools={tools} open={isExpanded(group)} onOpenChange={(open) => setGroupExpanded(group, open)} groupRef={index === 0 ? firstMatchRef : undefined} />)}
    {!filtered.length && <p className="duplicate-empty">{groups?.length ? "No groups match this filter. Your selection is unchanged." : tools.duplicateScan?.cancelled ? "No verified groups are available from this partial scan. Scan again to compare the remaining files." : "No identical file groups were found in the returned scan scope."}</p>}
    {filtered.length > visibleCount && <Button size="sm" variant="outline" onClick={() => setVisibleCount((count) => count + 50)}>Show more groups ({filtered.length - visibleCount} remaining)</Button>}
  </section>;
}

function DuplicateGroupRows({ group, tools, open, onOpenChange, groupRef }: { group: DuplicateGroup; tools: Tools; open: boolean; onOpenChange: (open: boolean) => void; groupRef?: React.RefObject<HTMLDetailsElement | null> }) {
  const [visibleCount, setVisibleCount] = useState(50);
  const selectedCount = group.files.filter((file) => tools.selected.has(file.id)).length;
  const retainedId = tools.retainedFileId(group);
  return <details ref={groupRef} className="duplicate-group" open={open} onToggle={(event) => onOpenChange(event.currentTarget.open)}>
    <summary>
      <Icon icon={fileType(group.files[0]?.name ?? "")?.icon ?? "document"} size={20} />
      <span className="duplicate-group-heading"><strong>{group.files[0]?.name}</strong><small>{group.fileCount} identical copies · {formatBytes(group.size)} each · {formatBytes(group.reclaimableBytes)} potential duplicate data</small></span>
      {selectedCount > 0 && <Badge tone="accent">{selectedCount} selected</Badge>}<Icon icon="chevron-down" size={14} />
    </summary>
    <div className="duplicate-members">
      {group.files.slice(0, visibleCount).map((file) => {
        const retained = retainedId === file.id;
        const displayPath = toExplorerDisplayPath(file.path) ?? "Location unavailable";
        // Older already-running backends do not include `removable`; treat the
        // missing field as an ordinary reviewable file until the next rebuild.
        const protectedForReview = file.removable === false;
        return <div key={file.id} className="duplicate-member" data-retained={retained || undefined}>
          <CheckboxControl checked={tools.selected.has(file.id)} disabled={tools.busy || retained || protectedForReview} ariaLabel={`${tools.selected.has(file.id) ? "Deselect" : "Select"} ${file.name} at ${displayPath}`} onChange={() => tools.select(file.id)} />
          <span className="duplicate-member-location"><strong>{file.name}</strong><small title={displayPath}>{displayPath} · {formatModified(file.modified)}</small></span>
          <span className="duplicate-member-size">{formatBytes(file.size)}</span>
          <Button size="sm" variant="ghost" disabled={tools.busy} aria-label={`Open ${file.name} in Explorer`} onClick={() => void tools.openDuplicate(file.id)}><Icon icon="folder-open" size={14} /> Open</Button>
          {protectedForReview ? <Badge tone="neutral">Protected · review only</Badge> : retained ? <Badge tone="success">Kept copy</Badge> : <Button size="sm" variant="ghost" disabled={tools.busy} aria-label={`Keep ${file.name} at ${displayPath}`} onClick={() => tools.keepFile(group.id, file.id)}>Keep selected</Button>}
        </div>;
      })}
      {group.files.length > visibleCount && <Button size="sm" variant="ghost" onClick={() => setVisibleCount((count) => count + 50)}>Inspect more copies ({group.files.length - visibleCount} remaining)</Button>}
      {(group.hasMoreFiles || group.memberOffset > 0) && <div className="flex flex-wrap items-center justify-between gap-2 py-2 text-xs text-[var(--text-dim)]"><span>{group.files.length} copies loaded of {group.fileCount}; original copy repeated for retention. Paging clears selections.</span><div className="flex gap-2"><Button size="sm" variant="outline" disabled={tools.busy || group.memberOffset === 0} onClick={() => void tools.loadMembers(group.id, Math.max(0, group.memberOffset - 200))}>Previous copies</Button><Button size="sm" variant="outline" disabled={tools.busy || !group.hasMoreFiles} onClick={() => void tools.loadMembers(group.id, group.memberOffset + 200)}>Next copies</Button></div></div>}
    </div>
  </details>;
}
