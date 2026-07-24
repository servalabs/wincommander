import { useEffect, useState, useCallback, useMemo } from "react";
import { Icon, Button, Tag, Spinner, InputGroup, Tooltip } from "@/components/ui/bp";
import { invoke } from "@tauri-apps/api/core";
import SectionCard from "../../shared/SectionCard";
import { executeBackendCommand } from "../../../hooks/useBackend";
import { showSuccess, showError } from "../../../utils/toast";
import { enabledRowsFirst } from "./systemManagerSort";
import "./SystemManagers.css";
import { AnimatedList, AnimatedTableRow, staggerDelay } from "../../shared/AnimatedList";

// Mirrors the PSCustomObject shape returned by Get-StartupItems
// (src-tauri/commander-free/scripts/modules/tweaks/startup-manager.ps1).
interface StartupItem {
    Name: string;
    Command: string;
    RamUsageMB: number;
    Status: string;          // "Running" | "Stopped" | "Disabled"
    IsEnabled: boolean;
    Source: string;          // "Registry" | "Folder" | "File" (backup)
    Location: string;
    Recommendation: "Keep" | "Disable" | "Neutral";
    Category: string;
    Description: string;
}

function extractCommandPath(command: string): string | null {
    const trimmed = command.trim();
    if (!trimmed) return null;
    const quoted = trimmed.match(/^"([^"]+)"/);
    if (quoted?.[1]) return quoted[1];
    const exe = trimmed.match(/^([A-Z]:\\[^\s]+?\.(?:exe|lnk|bat|cmd|ps1))/i);
    if (exe?.[1]) return exe[1];
    return null;
}

function parentFolder(path: string): string {
    const clean = path.replace(/\\+$/, "");
    const idx = clean.lastIndexOf("\\");
    return idx > 0 ? clean.slice(0, idx) : clean;
}

export default function StartupManager({ embedded = false }: { embedded?: boolean }) {
    // Collapsed by default — the data fetch only fires on first open so
    // the Tweaks panel doesn't pay a Get-StartupItems round-trip on every
    // mount (these subpanels stack vertically and were dominating load
    // time on slow machines).
    const [isOpen, setIsOpen] = useState(false);
    const [items, setItems] = useState<StartupItem[]>([]);
    const [loading, setLoading] = useState(false);
    const [pending, setPending] = useState<Set<string>>(new Set());
    const [filter, setFilter] = useState("");

    const refresh = useCallback(async () => {
        setLoading(true);
        const res = await executeBackendCommand<StartupItem[] | { error?: boolean; message?: string }>("Get-StartupItems");
        setLoading(false);
        if (res.success && Array.isArray(res.data)) {
            setItems(res.data);
        } else if (!res.success) {
            showError(res.error || "Failed to load startup items");
        }
    }, []);

    // Fetch on first expand; subsequent re-opens reuse the cache (user
    // can click the refresh icon for a re-probe).
    useEffect(() => {
        if ((isOpen || embedded) && items.length === 0 && !loading) refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isOpen, embedded]);

    const handleToggle = useCallback(async (item: StartupItem) => {
        setPending(prev => new Set(prev).add(item.Name));
        const cmd = item.IsEnabled ? "Disable-StartupItem" : "Enable-StartupItem";
        const res = await executeBackendCommand<{ success?: boolean; message?: string }>(cmd, {
            Name: item.Name,
            Location: item.Location,
        });
        setPending(prev => {
            const next = new Set(prev);
            next.delete(item.Name);
            return next;
        });
        if (res.success && res.data?.success !== false) {
            showSuccess(`${item.IsEnabled ? "Disabled" : "Enabled"} ${item.Name}`);
            refresh();
        } else {
            showError(res.error || res.data?.message || "Failed to toggle startup item");
        }
    }, [refresh]);

    const filtered = useMemo(() => {
        const q = filter.trim().toLowerCase();
        const matches = !q ? items : items.filter(i =>
            i.Name.toLowerCase().includes(q) ||
            (i.Description || "").toLowerCase().includes(q) ||
            (i.Category || "").toLowerCase().includes(q));
        return enabledRowsFirst(matches, item => item.IsEnabled);
    }, [items, filter]);

    const totalRam = useMemo(
        () => items.filter(i => i.Status === "Running").reduce((s, i) => s + (i.RamUsageMB || 0), 0),
        [items],
    );

    const headerRight = (
        <>
            <span className="system-manager-caption">Startup apps, live signal, memory cost, source, and command path.</span>
            <div className="system-manager-actions">
                <Tag minimal>{`${items.length} entries`}</Tag>
                <Tag minimal intent="warning">{`${totalRam.toFixed(0)} MB running`}</Tag>
                <Button minimal icon="refresh" onClick={(e) => { e.stopPropagation(); refresh(); }} loading={loading} />
            </div>
        </>
    );

    const body = (
        <>
            <InputGroup
                placeholder="Search startup items..."
                leftIcon="search"
                value={filter}
                onChange={e => setFilter(e.currentTarget.value)}
                className="system-manager-filter"
            />
            <AnimatedList className="system-manager-list system-manager-list--startup">
                {loading && items.length === 0 && <Spinner size={20} />}
                {!loading && filtered.length === 0 && (
                    <div style={{ color: "var(--color-text-muted)", padding: 12, fontSize: 12 }}>
                        No startup items match.
                    </div>
                )}
                {filtered.map((item, idx) => {
                    const isPending = pending.has(item.Name);
                    const commandPath = extractCommandPath(item.Command);
                    const rowKey = item.Location + "::" + item.Name;
                    // "Neutral" backend value is unclear in the UI — relabel to
                    // "Optional" with an explanatory tooltip so users know it
                    // means the call is theirs (no strong keep/disable signal).
                    const recIntent =
                        item.Recommendation === "Keep" ? "success" :
                        item.Recommendation === "Disable" ? "warning" : "none";
                    const recLabel = item.Recommendation === "Neutral" ? "Optional" : item.Recommendation;
                    const recTip =
                        item.Recommendation === "Keep" ? "Recommended to keep enabled — important for normal use." :
                        item.Recommendation === "Disable" ? "Recommended to disable — speeds up boot, not needed at startup." :
                        "No strong recommendation — keep or disable based on whether you use this app at startup.";
                    return (
                        <AnimatedTableRow
                            key={rowKey}
                            layoutId={rowKey}
                            entranceDelay={staggerDelay(idx)}
                        >
                            <div
                                className="system-manager-row system-manager-row--startup"
                                style={{
                                    // opacity fade when disabled — compositor-friendly, no reflow
                                    opacity: item.IsEnabled ? 1 : 0.55,
                                    transition: "opacity var(--dur-normal) var(--ease)",
                                }}
                            >
                                <div style={{ minWidth: 0 }}>
                                    <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
                                        <Icon icon={item.Source === "Folder" ? "folder-close" : "database"} size={12} />
                                        <span style={{ fontWeight: 600, fontSize: 13, minWidth: 0, overflowWrap: "anywhere" }}>{item.Name}</span>
                                        {item.Description !== "Unknown Application" && (
                                            <span style={{
                                                fontSize: 11,
                                                color: "var(--color-text-muted)",
                                                overflow: "hidden",
                                                textOverflow: "ellipsis",
                                                whiteSpace: "nowrap",
                                                minWidth: 0,
                                            }}>· {item.Description}</span>
                                        )}
                                    </div>
                                    {/* Command / path — folded to two lines max so a long
                                        install path doesn't push the row sideways out of the
                                        System Managers card (previously single-line ellipsis
                                        sometimes still spilled when the grid couldn't shrink). */}
                                    <button
                                        type="button"
                                        className="system-manager-path"
                                        onClick={() => commandPath && invoke("open_path", { path: parentFolder(commandPath) }).catch(() => {})}
                                        disabled={!commandPath}
                                        title={commandPath ? `${item.Command}\nClick to open containing folder` : item.Command}
                                    >
                                        {item.Command}
                                    </button>
                                </div>
                                <Tooltip content={recTip}>
                                    <Tag minimal intent={recIntent as any}>{recLabel}</Tag>
                                </Tooltip>
                                <Tag minimal>{item.Status === "Running" ? `${item.RamUsageMB.toFixed(0)} MB` : item.Status}</Tag>
                                <Button
                                    small
                                    intent={item.IsEnabled ? "danger" : "success"}
                                    loading={isPending}
                                    onClick={() => handleToggle(item)}
                                    icon={item.IsEnabled ? "disable" : "tick"}
                                >
                                    {item.IsEnabled ? "Disable" : "Enable"}
                                </Button>
                            </div>
                        </AnimatedTableRow>
                    );
                })}
            </AnimatedList>
        </>
    );

    if (embedded) {
        return (
            <div className="system-manager-embedded">
                <div className="system-manager-embedded__meta">{headerRight}</div>
                {body}
            </div>
        );
    }

    return (
        <SectionCard
            title="Startup Manager"
            icon="play"
            collapsible
            isOpen={isOpen}
            onToggle={() => setIsOpen(v => !v)}
            headerRight={headerRight}
        >
            {body}
        </SectionCard>
    );
}
