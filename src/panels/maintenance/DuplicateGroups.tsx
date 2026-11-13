import { useEffect, useMemo, useRef, useState } from "react";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { CheckboxControl } from "../../components/ui/bp";
import { Icon, type IconName } from "../../components/ui/icon";
import type { DuplicateGroup } from "../../hooks/useBackend";
import { formatBytes } from "./routineCleanerHelpers";
import type { useFileHygiene } from "./useFileHygiene";
import { toExplorerDisplayPath } from "./maintenancePathDisplay";
import { hasExpandedDuplicateGroups, isDuplicateGroupExpanded, setDuplicateGroupExpanded, toggleDuplicateGroups } from "./duplicateGroupPresentation";
import { duplicateProfileKey, duplicateProfileLabel, isDuplicateGroupsFiltered } from "./duplicateProfile";
import { isGitManagedDuplicate } from "../../lib/maintenanceSelection";
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

function reviewReason(reason?: string | null): string {
  switch (reason) {
    case "other_profile": return "Another user's files are protected";
    case "protected_location": return "Windows or application location is protected";
    case "protected_type": return "Application or executable files are protected";
    case "application_data": return "Application data is protected";
    case "source_control": return "Git-managed data is protected";
    case "profiles_unavailable": return "Windows profiles could not be verified";
    case "owner_unavailable": return "File ownership could not be verified";
    default: return "Removal eligibility has not been verified";
  }
}

function formatModified(modified: string | null) {
  if (!modified || !/^\d+$/.test(modified)) return "Modified date unavailable";
  try {
    const milliseconds = Number(BigInt(modified) / 1_000_000n);
    if (!Number.isFinite(milliseconds)) return "Modified date unavailable";
    return `Modified ${new Date(milliseconds).toLocaleString()}`;
  } catch { return "Modified date unavailable"; }
}

export function DuplicateGroups({ tools, onFilterChange, onRemoveFile }: { tools: Tools; onFilterChange: (active: boolean) => void; onRemoveFile: (fileId: string) => void }) {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("all");
  // Show all scanned locations, including copies protected from removal.
  const [profile, setProfile] = useState("all");
  const [visibleCount, setVisibleCount] = useState(50);
  const [expandedGroupIds, setExpandedGroupIds] = useState<Set<string> | null>(null);
  const firstMatchRef = useRef<HTMLDetailsElement>(null);
  const groups = tools.duplicateScan?.groups;
  const profiles = useMemo(() => {
    const choices = new Map<string, string>();
    for (const group of groups ?? []) for (const file of group.files) choices.set(duplicateProfileKey(file), duplicateProfileLabel(file));
    return [...choices.entries()].sort((a, b) => a[1].localeCompare(b[1]));
  }, [groups]);
  useEffect(() => {
    if (profile !== "all" && profile !== "current" && !profiles.some(([key]) => key === profile)) setProfile("all");
  }, [profile, profiles]);
  useEffect(() => {
    onFilterChange(isDuplicateGroupsFiltered(query, category, profile));
    return () => onFilterChange(false);
  }, [category, onFilterChange, profile, query]);
  const filtered = useMemo(() => (groups ?? []).filter((group) => {
    const files = group.files;
    if (!files.length) return (group.hasMoreFiles || group.memberOffset > 0) && category === "all" && !query.trim();
    return (category === "all" || files.some((file) => (fileType(file.name)?.id ?? "other") === category)) &&
    (profile === "all" || files.some((file) => (profile === "current" ? file.profileScope === "current" : duplicateProfileKey(file) === profile))) &&
    files.some((file) => `${file.name} ${toExplorerDisplayPath(file.path) ?? ""}`.toLowerCase().includes(query.trim().toLowerCase()));
  }), [category, groups, profile, query]);
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
    {groups?.some((group) => group.files.some((file) => file.removalBlockReason === "profiles_unavailable")) && <p role="status" className="text-xs text-[var(--warn)]">Windows user profile locations could not be verified, so removal is unavailable. Reconnect any offline profile drives or ask an administrator, then scan again.</p>}
    {groups?.some(group => group.files.some(file => file.removable === undefined)) && <p role="status" className="text-xs text-[var(--warn)]">Removal eligibility could not be verified for some copies. Update the app service and scan again.</p>}
    <form className="duplicate-filters" onSubmit={onSearch}>
      <label><Icon icon="search" size={14} /><input aria-label="Search duplicate files and locations" value={query} onChange={(event) => { setQuery(event.target.value); setVisibleCount(50); }} placeholder="Search file names or locations" /></label>
      <select aria-label="Filter duplicate groups by user profile" value={profile} onChange={(event) => { setProfile(event.target.value); setVisibleCount(50); }}>
        <option value="all">All scanned locations</option><option value="current">Current profile</option>{profiles.filter(([key]) => !key.startsWith("current:")).map(([key, label]) => <option key={key} value={key}>{label}</option>)}
      </select>
      <span>{filtered.length} of {groups?.length ?? 0} loaded groups · filters apply to this page</span>
      <Button size="sm" variant="outline" type="button" disabled={!filtered.length} onClick={toggleAll}>{hasExpandedGroups ? "Collapse all" : "Expand all"}</Button>
    </form>
    <div className="duplicate-categories" role="group" aria-label="Filter duplicate file type">
      {[{ id: "all", label: "All files" }, ...TYPES, { id: "other", label: "Other" }].map((type) =>
        <button key={type.id} type="button" className="duplicate-category" aria-pressed={category === type.id} onClick={() => { setCategory(type.id); setVisibleCount(50); }}>{type.label}</button>)}
    </div>
    <p className="text-xs text-[var(--text-dim)]">Filters apply to loaded groups only. Protected copies are shown for review and cannot be selected for removal.</p>
    {filtered.slice(0, visibleCount).map((group, index) => <DuplicateGroupRows key={groupKey(group)} group={group} tools={tools} onRemoveFile={onRemoveFile} open={isExpanded(group)} onOpenChange={(open) => setGroupExpanded(group, open)} groupRef={index === 0 ? firstMatchRef : undefined} />)}
    {!filtered.length && <p className="duplicate-empty">{groups?.length ? "No groups match this filter. Your selection is unchanged." : tools.duplicateScan?.cancelled ? "No verified groups are available from this partial scan. Scan again to compare the remaining files." : "No identical file groups were found in the returned scan scope."}</p>}
    {filtered.length > visibleCount && <Button size="sm" variant="outline" onClick={() => setVisibleCount((count) => count + 50)}>Show more groups ({filtered.length - visibleCount} remaining)</Button>}
  </section>;
}

function DuplicateGroupRows({ group, tools, onRemoveFile, open, onOpenChange, groupRef }: { group: DuplicateGroup; tools: Tools; onRemoveFile: (fileId: string) => void; open: boolean; onOpenChange: (open: boolean) => void; groupRef?: React.RefObject<HTMLDetailsElement | null> }) {
  const [visibleCount, setVisibleCount] = useState(50);
  const [showGitCopies, setShowGitCopies] = useState(false);
  const selectedCount = group.files.filter((file) => tools.selected.has(file.id)).length;
  const retainedId = tools.retainedFileId(group);
  const gitCopies = group.files.filter((file) => isGitManagedDuplicate(file.path));
  const visibleFiles = showGitCopies ? group.files : group.files.filter((file) => !isGitManagedDuplicate(file.path));
  const displayFile = group.files.find((file) => !isGitManagedDuplicate(file.path)) ?? group.files[0];
  return <details ref={groupRef} className="duplicate-group" open={open} onToggle={(event) => onOpenChange(event.currentTarget.open)}>
    <summary>
      <Icon icon={fileType(displayFile?.name ?? "")?.icon ?? "document"} size={20} />
      <span className="duplicate-group-heading"><strong>{displayFile?.name ?? "Duplicate copies"}</strong><small>{group.fileCount} full-content hash matches including protected · {formatBytes(group.size)} each · {formatBytes(group.reclaimableBytes)} potential duplicate data</small></span>
      {selectedCount > 0 && <Badge tone="accent">{selectedCount} selected</Badge>}
      <Button type="button" size="sm" variant="outline" disabled={tools.busy || !visibleFiles.some(file => file.removable === true && !isGitManagedDuplicate(file.path) && file.id !== retainedId)} onClick={(event) => { event.preventDefault(); event.stopPropagation(); tools.selectGroupCopies(group.id); }}>Select copies</Button>
      <Icon icon="chevron-down" size={14} />
    </summary>
    <div className="duplicate-members">
      {gitCopies.length > 0 && <Button type="button" size="sm" variant="ghost" aria-expanded={showGitCopies} onClick={() => setShowGitCopies((show) => !show)}>{showGitCopies ? "Hide" : "Show"} {gitCopies.length} Git-managed {gitCopies.length === 1 ? "copy" : "copies"} on this page · review only</Button>}
      {!visibleFiles.length && <p className="text-xs text-[var(--text-dim)]">{gitCopies.length ? "Only Git-managed copies are on this page." : "No copies on this page. Check other pages for this group."}</p>}
      {visibleFiles.slice(0, visibleCount).map((file) => {
        const retained = retainedId === file.id;
        const gitManaged = isGitManagedDuplicate(file.path);
        const protectedCopy = gitManaged || file.removable !== true;
        const displayPath = toExplorerDisplayPath(file.path) ?? "Location unavailable";
        return <div key={file.id} className="duplicate-member" data-retained={retained || undefined}>
          <CheckboxControl checked={!protectedCopy && tools.selected.has(file.id)} disabled={tools.busy || retained || protectedCopy} ariaLabel={`${tools.selected.has(file.id) ? "Deselect" : "Select"} ${file.name} at ${displayPath}`} onChange={() => tools.select(file.id)} />
          <span className="duplicate-member-location"><strong>{file.name}</strong><small title={displayPath}>{displayPath} · {formatModified(file.modified)} · {duplicateProfileLabel(file)}</small></span>
          <Button size="sm" variant="ghost" disabled={tools.busy} aria-label={`Open ${file.name} in Explorer`} onClick={() => void tools.openDuplicate(file.id)}><Icon icon="folder-open" size={14} /></Button>
          {protectedCopy ? <span title={gitManaged ? "Git-managed data is protected" : reviewReason(file.removalBlockReason)}><Badge tone="neutral">{retained ? "Kept · review only" : "Review only"}</Badge></span> : retained ? <Badge tone="success">Kept copy</Badge> : <><Button size="sm" variant="ghost" disabled={tools.busy} aria-label={`Keep ${file.name} at ${displayPath}`} onClick={() => tools.keepFile(group.id, file.id)}>Keep</Button><Button size="sm" variant="outline" disabled={tools.busy} aria-label={`Remove ${file.name} at ${displayPath}`} title="Remove file" onClick={() => onRemoveFile(file.id)}><Icon icon="trash" size={14} /></Button></>}
        </div>;
      })}
      {visibleFiles.length > visibleCount && <Button size="sm" variant="ghost" onClick={() => setVisibleCount((count) => count + 50)}>Inspect more copies ({visibleFiles.length - visibleCount} remaining)</Button>}
      {(group.hasMoreFiles || group.memberOffset > 0) && <div className="flex flex-wrap items-center justify-between gap-2 py-2 text-xs text-[var(--text-dim)]"><span>{group.files.length} copies loaded of {group.fileCount}; original copy repeated for retention. Paging clears selections.</span><div className="flex gap-2"><Button size="sm" variant="outline" disabled={tools.busy || group.memberOffset === 0} onClick={() => void tools.loadMembers(group.id, Math.max(0, group.memberOffset - 200))}>Previous copies</Button><Button size="sm" variant="outline" disabled={tools.busy || !group.hasMoreFiles} onClick={() => void tools.loadMembers(group.id, group.memberOffset + 200)}>Next copies</Button></div></div>}
    </div>
  </details>;
}
