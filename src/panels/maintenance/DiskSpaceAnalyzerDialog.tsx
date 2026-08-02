import { useState, useCallback, useEffect, useRef, useMemo } from "react";
import { Classes, Dialog, Spinner, Icon, Button, Alert, CheckboxControl } from "@/components/ui/bp";
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog";
import useBackend from "../../hooks/useBackend";
import { showError, showSuccess } from "../../utils/toast";
import { ui } from "@/assets";

// ── Types ─────────────────────────────────────────────────────────────────────

interface DiskNode {
    name: string;
    fullPath: string;
    size: number;
    allocated: number;
    isDir: boolean;
    lastModified: string;
    fileCount: number;
    folderCount: number;
}

interface LargeDiskItem extends DiskNode {
    itemType: string;
    cleanupHint: string;
    risk: string;
}

interface ScanMeta {
    scanRoot: string;
    totalSize: number;
    freeSpace: number;
    driveCapacity: number;
    fileCount: number;
    folderCount: number;
}

interface Props {
    isOpen: boolean;
    onClose: () => void;
    initialMode?: "space" | "large";
    /** When true, render the analyzer body inline (no Dialog wrapper) so it
     *  can live as a section on a panel page. `isOpen` is ignored in this
     *  mode and the UI is always visible. */
    inline?: boolean;
}

interface DiskAnalyzerSession {
    scanning: boolean;
    scanPath: string;
    meta: ScanMeta | null;
    rows: DiskNode[];
    largeItems: LargeDiskItem[];
    breadcrumbs: string[];
    inFlight: Promise<void> | null;
}

let diskAnalyzerSession: DiskAnalyzerSession = {
    scanning: false,
    scanPath: "C:\\",
    meta: null,
    rows: [],
    largeItems: [],
    breadcrumbs: [],
    inFlight: null,
};

const diskAnalyzerSubscribers = new Set<(session: DiskAnalyzerSession) => void>();

function snapshotDiskAnalyzerSession(): DiskAnalyzerSession {
    return {
        ...diskAnalyzerSession,
        rows: [...diskAnalyzerSession.rows],
        largeItems: [...diskAnalyzerSession.largeItems],
        breadcrumbs: [...diskAnalyzerSession.breadcrumbs],
    };
}

function persistDiskAnalyzerSession(patch: Partial<DiskAnalyzerSession>) {
    diskAnalyzerSession = { ...diskAnalyzerSession, ...patch };
    const snapshot = snapshotDiskAnalyzerSession();
    diskAnalyzerSubscribers.forEach((listener) => listener(snapshot));
}

function subscribeDiskAnalyzerSession(listener: (session: DiskAnalyzerSession) => void) {
    diskAnalyzerSubscribers.add(listener);
    listener(snapshotDiskAnalyzerSession());
    return () => {
        diskAnalyzerSubscribers.delete(listener);
    };
}


function getFileIcon(name: string, isDir: boolean) {
    if (isDir) return { icon: "folder-close" as any, colorClass: "da-icon-folder" };
    const ext = name.split(".").pop()?.toLowerCase() || "";
    if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico"].includes(ext)) return { icon: "media" as any, colorClass: "da-icon-image" };
    if (["mp4", "mkv", "mov", "avi", "wmv", "flv", "webm"].includes(ext)) return { icon: "video" as any, colorClass: "da-icon-video" };
    if (["mp3", "wav", "flac", "m4a", "ogg", "aac"].includes(ext)) return { icon: "music" as any, colorClass: "da-icon-audio" };
    if (["zip", "rar", "7z", "tar", "gz", "iso", "cab"].includes(ext)) return { icon: "compressed" as any, colorClass: "da-icon-archive" };
    if (["pdf", "doc", "docx", "txt", "md", "rtf"].includes(ext)) return { icon: "document" as any, colorClass: "da-icon-doc" };
    if (["js", "ts", "tsx", "jsx", "py", "cpp", "rs", "html", "css", "json", "yml", "yaml", "sh", "bat", "ps1"].includes(ext)) return { icon: "code" as any, colorClass: "da-icon-code" };
    if (["xls", "xlsx", "csv"].includes(ext)) return { icon: "th" as any, colorClass: "da-icon-sheet" };
    if (["exe", "msi", "appx", "ps1", "bat", "cmd"].includes(ext)) return { icon: "application" as any, colorClass: "da-icon-exe" };
    return { icon: "document" as any, colorClass: "da-icon-file" };
}


// ── Disk size helpers ─────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
    if (bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return `${(bytes / Math.pow(1024, i)).toFixed(i >= 2 ? 1 : 0)} ${units[i]}`;
}
function pct(size: number, total: number): number {
    if (!total) return 0;
    return Math.min(100, Math.round((size / total) * 100));
}
function SizeBar({ value, max }: { value: number; max: number }) {
    const p = pct(value, max);
    const color = p > 80 ? "#e05252" : p > 50 ? "#e0a040" : "#4a9f6e";
    return (
        <div style={{ background: "rgba(255,255,255,0.07)", borderRadius: 2, height: 6, width: "100%", overflow: "hidden" }}>
            <div style={{ width: `${p}%`, height: "100%", background: color, borderRadius: 2, transition: "width 0.3s" }} />
        </div>
    );
}


function PageFlipAnimation({ size = 64 }: { size?: number }) {
    return (
        <div className="da-searching-gif-wrap" style={{ width: size, height: size }}>
            <img src={ui["searching.gif"]} alt="" className="da-searching-gif" />
        </div>
    );
}

// ── Squarified treemap ────────────────────────────────────────────────────────

interface TileRect { x: number; y: number; w: number; h: number; item: DiskNode; }

function squarify(items: DiskNode[], W: number, H: number): TileRect[] {
    const valid = items.filter(i => i.size > 0).sort((a, b) => b.size - a.size);
    if (!valid.length || W < 1 || H < 1) return [];
    const total = valid.reduce((s, i) => s + i.size, 0);
    const rects: TileRect[] = [];

    // Pre-compute normalised pixel-areas
    const nodes = valid.map(i => ({ ...i, area: (i.size / total) * W * H }));

    function worstRatio(row: typeof nodes, bandLen: number): number {
        let worst = 0;
        const rowArea = row.reduce((s, n) => s + n.area, 0);
        const h = rowArea / bandLen;
        for (const n of row) {
            const w = n.area / h;
            const r = Math.max(h / w, w / h);
            if (r > worst) worst = r;
        }
        return worst;
    }

    function layout(ns: typeof nodes, x: number, y: number, W: number, H: number) {
        if (ns.length === 0) return;
        if (ns.length === 1) { rects.push({ x, y, w: W, h: H, item: ns[0] }); return; }

        const horizontal = W >= H;
        const shortSide = horizontal ? H : W;
        let row: typeof nodes = [];
        let i = 0;

        while (i < ns.length) {
            const prev = row.length > 0 ? worstRatio(row, shortSide) : Infinity;
            const next = worstRatio([...row, ns[i]], shortSide);
            if (row.length > 0 && next > prev) break;
            row.push(ns[i++]);
        }

        const rowArea = row.reduce((s, n) => s + n.area, 0);
        const bandW = horizontal ? rowArea / H : W;
        const bandH = horizontal ? H : rowArea / W;
        let pos = horizontal ? y : x;

        for (const n of row) {
            const frac = n.area / rowArea;
            const tw = horizontal ? bandW : W * frac;
            const th = horizontal ? H * frac : bandH;
            rects.push({ x: horizontal ? x : pos, y: horizontal ? pos : y, w: tw, h: th, item: n });
            pos += horizontal ? th : tw;
        }

        const rest = ns.slice(i);
        if (rest.length)
            layout(rest, horizontal ? x + bandW : x, horizontal ? y : y + bandH,
                horizontal ? W - bandW : W, horizontal ? H : H - bandH);
    }

    layout(nodes, 0, 0, W, H);
    return rects;
}

// Colour-code tiles by extension group (WizTree-like palette)
function getTileColor(node: DiskNode): string {
    if (node.isDir) return "rgba(59, 130, 246, 0.55)";
    const ext = node.name.split(".").pop()?.toLowerCase() ?? "";
    if (["mp4","mkv","mov","avi","wmv","webm","m4v"].includes(ext)) return "rgba(234, 179, 8, 0.58)";
    if (["jpg","jpeg","png","gif","webp","bmp","heic","svg"].includes(ext)) return "rgba(168, 85, 247, 0.55)";
    if (["mp3","wav","flac","m4a","ogg","aac"].includes(ext)) return "rgba(14, 165, 233, 0.55)";
    if (["zip","rar","7z","tar","gz","iso","cab"].includes(ext)) return "rgba(249, 115, 22, 0.55)";
    if (["exe","msi","appx","msix"].includes(ext)) return "rgba(239, 68, 68, 0.55)";
    if (["pdf","doc","docx","xls","xlsx","ppt","pptx"].includes(ext)) return "rgba(74, 222, 128, 0.52)";
    return "rgba(148, 163, 184, 0.40)";
}
function getTileBorder(node: DiskNode): string {
    if (node.isDir) return "rgba(147, 197, 253, 0.6)";
    const ext = node.name.split(".").pop()?.toLowerCase() ?? "";
    if (["mp4","mkv","mov","avi","wmv","webm","m4v"].includes(ext)) return "rgba(253, 224, 71, 0.65)";
    if (["jpg","jpeg","png","gif","webp","bmp","heic","svg"].includes(ext)) return "rgba(216, 180, 254, 0.65)";
    if (["mp3","wav","flac","m4a","ogg","aac"].includes(ext)) return "rgba(125, 211, 252, 0.65)";
    if (["zip","rar","7z","tar","gz","iso","cab"].includes(ext)) return "rgba(253, 186, 116, 0.65)";
    if (["exe","msi","appx","msix"].includes(ext)) return "rgba(252, 165, 165, 0.65)";
    return "rgba(203, 213, 225, 0.45)";
}

function SquarifiedTreemap({ items, totalSize, onDrill, onOpen }: {
    items: DiskNode[]; totalSize: number;
    onDrill: (n: DiskNode) => void; onOpen: (path: string) => void;
}) {
    const containerRef = useRef<HTMLDivElement>(null);
    const [dims, setDims] = useState({ w: 600, h: 220 });
    const [pan, setPan] = useState({ x: 0, y: 0 });
    const [panTransition, setPanTransition] = useState(false);
    const dragRef = useRef<{ mx: number; my: number; px: number; py: number } | null>(null);
    const [isDragging, setIsDragging] = useState(false);
    const didDragRef = useRef(false);
    const idleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const [infoItem, setInfoItem] = useState<DiskNode | null>(null);

    useEffect(() => {
        const el = containerRef.current;
        if (!el) return;
        const ro = new ResizeObserver(entries => {
            const { width, height } = entries[0].contentRect;
            if (width > 0 && height > 0) setDims({ w: width, h: height });
        });
        ro.observe(el);
        return () => ro.disconnect();
    }, []);

    // Reset pan when items change (drill-in / drill-out)
    useEffect(() => {
        setPanTransition(false);
        setPan({ x: 0, y: 0 });
        setInfoItem(null);
    }, [items]);

    const cancelIdleTimer = () => {
        if (idleTimerRef.current) { clearTimeout(idleTimerRef.current); idleTimerRef.current = null; }
    };
    const scheduleIdleReset = () => {
        cancelIdleTimer();
        idleTimerRef.current = setTimeout(() => {
            setPanTransition(true);
            setPan({ x: 0, y: 0 });
            setTimeout(() => setPanTransition(false), 500);
        }, 4000);
    };

    useEffect(() => () => cancelIdleTimer(), []);

    const tiles = useMemo(() => squarify(items, dims.w, dims.h), [items, dims]);
    const GAP = 2;

    const onMouseDown = (e: React.MouseEvent) => {
        cancelIdleTimer();
        dragRef.current = { mx: e.clientX, my: e.clientY, px: pan.x, py: pan.y };
        didDragRef.current = false;
        setIsDragging(true);
        setPanTransition(false);
        e.preventDefault();
    };
    const onMouseMove = (e: React.MouseEvent) => {
        if (!dragRef.current) return;
        const dx = e.clientX - dragRef.current.mx;
        const dy = e.clientY - dragRef.current.my;
        if (Math.abs(dx) > 3 || Math.abs(dy) > 3) didDragRef.current = true;
        setPan({ x: dragRef.current.px + dx, y: dragRef.current.py + dy });
    };
    const onMouseUp = () => {
        dragRef.current = null;
        setIsDragging(false);
        scheduleIdleReset();
    };

    return (
        <div
            ref={containerRef}
            className="da-sq-treemap"
            style={{ cursor: isDragging ? "grabbing" : "grab" }}
            onMouseDown={onMouseDown}
            onMouseMove={onMouseMove}
            onMouseUp={onMouseUp}
            onMouseLeave={onMouseUp}
            onClick={(e) => { if ((e.target as HTMLElement).closest(".da-sq-info-box")) return; if (infoItem) setInfoItem(null); }}
        >
            {/* drag-hint badge */}
            <span className="da-sq-drag-hint">drag to pan</span>

            {/* Info overlay */}
            {infoItem && (
                <div className="da-sq-info-box" onClick={(e) => e.stopPropagation()}>
                    <div className="da-sq-info-header">
                        <Icon icon={infoItem.isDir ? "folder-close" : "document"} size={14} />
                        <span className="da-sq-info-name">{infoItem.name}</span>
                        <button type="button" className="da-sq-info-close" onClick={() => setInfoItem(null)} aria-label={`Close details for ${infoItem.name}`} title="Close details">×</button>
                    </div>
                    <div className="da-sq-info-path">{infoItem.fullPath}</div>
                    <div className="da-sq-info-stats">
                        <span><strong>{formatBytes(infoItem.size)}</strong> ({pct(infoItem.size, totalSize)}%)</span>
                        {infoItem.isDir && <span>{infoItem.fileCount.toLocaleString()} files · {infoItem.folderCount.toLocaleString()} folders</span>}
                        {infoItem.lastModified && <span>Modified {infoItem.lastModified}</span>}
                    </div>
                    <div className="da-sq-info-actions">
                        <Button small intent="none" icon="folder-shared-open" onClick={() => { onOpen(infoItem.fullPath); setInfoItem(null); }}>Open in Explorer</Button>
                        {infoItem.isDir && <Button small intent="primary" icon="zoom-in" onClick={() => { onDrill(infoItem); setInfoItem(null); }}>Drill in</Button>}
                    </div>
                </div>
            )}

            <div style={{
                transform: `translate(${pan.x}px,${pan.y}px)`,
                transition: panTransition ? "transform 0.45s cubic-bezier(0.4,0,0.2,1)" : "none",
                position: "absolute", inset: 0,
                pointerEvents: isDragging ? "none" : "auto",
            }}>
                {tiles.map(({ x, y, w, h, item }) => {
                    const gx = x + GAP / 2, gy = y + GAP / 2, gw = Math.max(0, w - GAP), gh = Math.max(0, h - GAP);
                    // Skip tiles too small to be useful — sub-6px tiles are invisible noise
                    if (gw < 6 || gh < 6) return null;
                    const showName = gw > 38 && gh > 20;
                    const showSize = gw > 50 && gh > 34;
                    const showMeta = gw > 70 && gh > 54 && item.isDir;
                    return (
                        <button
                            key={item.fullPath}
                            type="button"
                            className={`da-sq-tile${infoItem?.fullPath === item.fullPath ? " da-sq-tile--selected" : ""}`}
                            style={{
                                left: gx, top: gy, width: gw, height: gh,
                                background: getTileColor(item),
                                borderColor: getTileBorder(item),
                            }}
                            title={`${item.fullPath}\n${formatBytes(item.size)} (${pct(item.size, totalSize)}%)`}
                            aria-label={`${item.name}, ${formatBytes(item.size)}, ${pct(item.size, totalSize)} percent of this folder. Show details.`}
                            onClick={(e) => {
                                e.stopPropagation();
                                if (didDragRef.current) return;
                                setInfoItem(prev => prev?.fullPath === item.fullPath ? null : item);
                            }}
                        >
                            {showName && (
                                <span className="da-sq-name">
                                    <Icon icon={item.isDir ? "folder-close" : "document"} size={10} className={getFileIcon(item.name, item.isDir).colorClass} />
                                    {item.name}
                                </span>
                            )}
                            {showSize && <strong className="da-sq-size">{formatBytes(item.size)}</strong>}
                            {showMeta && (
                                <small className="da-sq-meta">{item.fileCount.toLocaleString()} files</small>
                            )}
                        </button>
                    );
                })}
            </div>
        </div>
    );
}



// ── Main Component ────────────────────────────────────────────────────────────

export default function DiskSpaceAnalyzerDialog({ isOpen, onClose, initialMode = "space", inline = false }: Props) {
    const { runDiskScan, getDiskChildren, getLargeDiskItems, diskDeleteItem, openPath: openPathBackend } = useBackend();

    // Tree state
    const [scanning, setScanning] = useState(diskAnalyzerSession.scanning);
    const [scanPath, setScanPath] = useState(diskAnalyzerSession.scanPath);
    const [meta, setMeta] = useState<ScanMeta | null>(diskAnalyzerSession.meta);
    const [rows, setRows] = useState<DiskNode[]>(diskAnalyzerSession.rows);
    const [loadingPath, setLoadingPath] = useState<string | null>(null);
    const [selected, setSelected] = useState<Set<string>>(new Set());
    const [showBulkDeleteConfirm, setShowBulkDeleteConfirm] = useState(false);
    const [deletingSingle, setDeletingSingle] = useState<DiskNode | null>(null);
    const [deleting, setDeleting] = useState(false);
    const breadcrumbs = useRef<string[]>(diskAnalyzerSession.breadcrumbs);
    const [viewMode, setViewMode] = useState<"space" | "large">(initialMode);
    const [spaceViewStyle, setSpaceViewStyle] = useState<"treemap" | "list">("treemap");
    const [largeItems, setLargeItems] = useState<LargeDiskItem[]>(diskAnalyzerSession.largeItems);
    const [largeFilter, setLargeFilter] = useState<"all" | "review" | "videos" | "archives" | "installers" | "folders">("all");
    const [minLargeSize, setMinLargeSize] = useState(100 * 1024 * 1024);

    useEffect(() => subscribeDiskAnalyzerSession((session) => {
        setScanning(session.scanning);
        setScanPath(session.scanPath);
        setMeta(session.meta);
        setRows(session.rows);
        setLargeItems(session.largeItems);
        breadcrumbs.current = session.breadcrumbs;
        if (session.scanning) {
            setSelected(new Set());
            setLoadingPath(null);
        }
    }), []);

    useEffect(() => {
        if (isOpen) setViewMode(initialMode);
    }, [isOpen, initialMode]);

    const setScanPathPersisted = useCallback((next: string) => {
        setScanPath(next);
        persistDiskAnalyzerSession({ scanPath: next });
    }, []);

    // ── Folder Picker ─────────────────────────────────────────────────────────

    const browseFolderPicker = useCallback(async () => {
        const sel = await openFolderDialog({ directory: true, multiple: false, title: "Select a Drive or Folder" });
        if (sel && typeof sel === "string") setScanPathPersisted(sel);
    }, [setScanPathPersisted]);

    // ── Disk Scan ─────────────────────────────────────────────────────────────

    const loadLargeItems = useCallback(async (minSize = minLargeSize) => {
        const items = await getLargeDiskItems(minSize, 300, true);
        setLargeItems(items);
        persistDiskAnalyzerSession({ largeItems: items });
    }, [getLargeDiskItems, minLargeSize]);

    const startScan = useCallback(async () => {
        if (diskAnalyzerSession.inFlight) {
            await diskAnalyzerSession.inFlight;
            return;
        }
        const requestedPath = scanPath;
        setSelected(new Set());
        persistDiskAnalyzerSession({
            scanning: true,
            scanPath: requestedPath,
            meta: null,
            rows: [],
            largeItems: [],
            breadcrumbs: [],
        });

        const scanTask = (async () => {
            try {
                const result = await runDiskScan(requestedPath);
                const nextRows = await getDiskChildren(result.scanRoot);
                const nextLargeItems = await getLargeDiskItems(minLargeSize, 300, true);
                persistDiskAnalyzerSession({
                    scanning: false,
                    meta: result,
                    rows: nextRows,
                    largeItems: nextLargeItems,
                    breadcrumbs: [result.scanRoot],
                    inFlight: null,
                });
            } catch (err: unknown) {
                persistDiskAnalyzerSession({ scanning: false, inFlight: null });
                showError(String(err));
            }
        })();

        persistDiskAnalyzerSession({ inFlight: scanTask });
        await scanTask;
    }, [scanPath, runDiskScan, getDiskChildren, getLargeDiskItems, minLargeSize]);

    // ── Navigation ────────────────────────────────────────────────────────────

    const loadDir = useCallback(async (path: string, newCrumbs: string[]) => {
        setLoadingPath(path);
        setSelected(new Set());
        try {
            const nextRows = await getDiskChildren(path);
            setRows(nextRows);
            breadcrumbs.current = newCrumbs;
            persistDiskAnalyzerSession({ rows: nextRows, breadcrumbs: newCrumbs });
        } catch (err: unknown) {
            showError(String(err));
        } finally {
            setLoadingPath(null);
        }
    }, [getDiskChildren]);

    const drillInto = useCallback((node: DiskNode) => {
        if (!node.isDir) return;
        loadDir(node.fullPath.replace(/\\+$/, ""), [...breadcrumbs.current, node.fullPath.replace(/\\+$/, "")]);
    }, [loadDir]);

    const navigateTo = useCallback((path: string, idx: number) => {
        loadDir(path, breadcrumbs.current.slice(0, idx + 1));
    }, [loadDir]);

    const goUp = useCallback(() => {
        const crumbs = breadcrumbs.current;
        if (crumbs.length <= 1) return;
        navigateTo(crumbs[crumbs.length - 2], crumbs.length - 2);
    }, [navigateTo]);

    // ── Selection ─────────────────────────────────────────────────────────────

    const toggleSelect = useCallback((path: string, e?: React.MouseEvent) => {
        e?.stopPropagation();
        setSelected(prev => { const n = new Set(prev); n.has(path) ? n.delete(path) : n.add(path); return n; });
    }, []);

    const toggleSelectAll = useCallback(() => {
        setSelected(prev => prev.size === rows.length ? new Set() : new Set(rows.map(r => r.fullPath)));
    }, [rows]);

    // ── Delete ────────────────────────────────────────────────────────────────

    const doDelete = useCallback(async (paths: string[]) => {
        setDeleting(true);
        const failed: string[] = [];
        for (const p of paths) {
            try { await diskDeleteItem(p); } catch { failed.push(p); }
        }
        const removed = new Set(paths.filter(p => !failed.includes(p)));
        const nextRows = diskAnalyzerSession.rows.filter(r => !removed.has(r.fullPath));
        const nextLargeItems = diskAnalyzerSession.largeItems.filter(r => !removed.has(r.fullPath));
        setRows(nextRows);
        setLargeItems(nextLargeItems);
        persistDiskAnalyzerSession({ rows: nextRows, largeItems: nextLargeItems });
        setSelected(prev => { const n = new Set(prev); removed.forEach(p => n.delete(p)); return n; });
        if (failed.length) showError(`Failed to delete ${failed.length} item(s)`);
        else showSuccess(`Deleted ${paths.length} item(s)`);
        setDeleting(false);
        setDeletingSingle(null);
        setShowBulkDeleteConfirm(false);
    }, [diskDeleteItem]);

    const openPath = useCallback(async (path: string) => {
        try { await openPathBackend(path); } catch { }
    }, [openPathBackend]);

    const openParentFolder = useCallback(async (path: string) => {
        const clean = path.replace(/\\+$/, "");
        const idx = clean.lastIndexOf("\\");
        await openPath(idx > 0 ? clean.slice(0, idx) : clean);
    }, [openPath]);

    // ── Render ────────────────────────────────────────────────────────────────

    const crumbs = breadcrumbs.current;
    const totalForBar = meta?.totalSize ?? 1;
    const allSelected = rows.length > 0 && selected.size === rows.length;
    const someSelected = selected.size > 0 && !allSelected;
    const canGoUp = crumbs.length > 1;
    const filteredLargeItems = largeItems.filter(item => {
        if (largeFilter === "all") return true;
        if (largeFilter === "review") return item.risk === "review";
        if (largeFilter === "videos") return item.itemType === "Video";
        if (largeFilter === "archives") return item.itemType === "Archive";
        if (largeFilter === "installers") return item.itemType === "Installer";
        if (largeFilter === "folders") return item.isDir;
        return true;
    });
    const selectedBytes = [...selected].reduce((sum, path) => {
        const item = [...rows, ...largeItems].find(r => r.fullPath === path);
        return sum + (item?.size ?? 0);
    }, 0);
    const sortedRows = rows.slice().sort((a, b) => b.size - a.size);
    const currentFolderSize = rows.reduce((sum, row) => sum + row.size, 0);
    /* Body content of the analyzer — same JSX whether we render as a modal
       (Dialog wrapper) or inline as a panel section. Toggled below via the
       `inline` prop. */
    const innerBody = (
        <div className="bp5-dialog-body" style={{ padding: 0, display: "flex", flexDirection: "column", flex: 1, overflow: "hidden", minHeight: 0 }}>

                    {/* ── Top Bar ── */}
                    <div className="da-topbar">
                        <div className="da-path-input-wrap">
                            <Icon icon="folder-open" size={16} />
                            <input
                                className="da-path-input"
                                aria-label="Folder or drive to analyse"
                                value={scanPath}
                                onChange={e => setScanPathPersisted(e.target.value)}
                                onKeyDown={e => e.key === "Enter" && startScan()}
                                placeholder="Drive or folder (e.g. C:\)"
                                spellCheck={false}
                            />
                        </div>
                        <Button icon="folder-open" title="Browse for folder" aria-label="Browse for folder to analyse" onClick={browseFolderPicker} disabled={scanning} className="da-browse-btn" minimal />
                        <Button
                            intent="primary"
                            icon={meta || largeItems.length ? "refresh" : "search"}
                            loading={scanning}
                            onClick={startScan}
                            disabled={scanning}
                            className="da-scan-btn"
                            title={meta || largeItems.length ? "Rescan disk usage" : "Scan disk usage"}
                            aria-label={meta || largeItems.length ? "Rescan disk usage" : "Scan disk usage"}
                        />
                    </div>

                    <div className="da-modebar">
                        <div className="da-mode-tabs">
                            <button type="button" aria-pressed={viewMode === "space"} className={viewMode === "space" ? "active" : ""} onClick={() => setViewMode("space")}>Disk Usage</button>
                            <button type="button" aria-pressed={viewMode === "large"} className={viewMode === "large" ? "active" : ""} onClick={() => setViewMode("large")}>Large Files</button>
                        </div>
                        {viewMode === "space" && rows.length > 0 && (
                            <div className="da-mode-tabs da-view-tabs">
                                <button type="button" aria-pressed={spaceViewStyle === "treemap"} className={spaceViewStyle === "treemap" ? "active" : ""} onClick={() => setSpaceViewStyle("treemap")}>Treemap</button>
                                <button type="button" aria-pressed={spaceViewStyle === "list"} className={spaceViewStyle === "list" ? "active" : ""} onClick={() => setSpaceViewStyle("list")}>List</button>
                            </div>
                        )}
                        {selected.size > 0 && (
                            <div className="da-selected-summary">
                                {selected.size} selected • {formatBytes(selectedBytes)}
                            </div>
                        )}
                    </div>

                    {/* ── Two-column body ── */}
                    <div className="da-body-columns">

                        {/* ── LEFT: Tree explorer ── */}
                        <div className="da-col-main">
                            {viewMode === "large" && (
                                <div className="da-large-toolbar">
                                    <div className="da-large-copy">
                                        <strong>Largest cleanup candidates</strong>
                                        <span>{meta ? `From ${meta.scanRoot}` : "Run a scan to inspect the selected drive or folder."}</span>
                                    </div>
                                    <div className="da-large-controls">
                                        {[100, 500, 1024].map(mb => (
                                            <button
                                                key={mb}
                                                type="button"
                                                aria-pressed={minLargeSize === mb * 1024 * 1024}
                                                className={minLargeSize === mb * 1024 * 1024 ? "active" : ""}
                                                onClick={async () => {
                                                    const next = mb * 1024 * 1024;
                                                    setMinLargeSize(next);
                                                    if (meta) await loadLargeItems(next);
                                                }}
                                            >
                                                &gt;{mb === 1024 ? "1 GB" : `${mb} MB`}
                                            </button>
                                        ))}
                                    </div>
                                </div>
                            )}

                            {/* Drive summary bar */}
                            {meta && (
                                <div className="da-summary">
                                    <div className="da-summary-stat">
                                        <span className="da-stat-label">Used</span>
                                        <span className="da-stat-val used">{formatBytes(meta.totalSize)}</span>
                                    </div>
                                    <div className="da-bar-wrap">
                                        <SizeBar value={meta.totalSize} max={meta.driveCapacity || meta.totalSize} />
                                        <div className="da-bar-labels">
                                            <span>{pct(meta.totalSize, meta.driveCapacity || meta.totalSize)}% used</span>
                                            <span>{formatBytes(meta.freeSpace)} free</span>
                                        </div>
                                    </div>
                                    <div className="da-summary-stat right">
                                        <span className="da-stat-label">Capacity</span>
                                        <span className="da-stat-val">{formatBytes(meta.driveCapacity || meta.totalSize)}</span>
                                    </div>
                                    <div className="da-summary-stat right">
                                        <span className="da-stat-label">Files</span>
                                        <span className="da-stat-val">{meta.fileCount.toLocaleString()}</span>
                                    </div>
                                    <div className="da-summary-stat right">
                                        <span className="da-stat-label">Folders</span>
                                        <span className="da-stat-val">{meta.folderCount.toLocaleString()}</span>
                                    </div>
                                </div>
                            )}

                            {/* Nav bar */}
                            {viewMode === "space" && crumbs.length > 0 && (
                                <div className="da-navbar">
                                    <Button icon="arrow-up" minimal small disabled={!canGoUp || !!loadingPath} onClick={goUp} className="da-up-btn" title="Go to parent directory" aria-label="Go to parent directory" />
                                    <div className="da-breadcrumbs">
                                        {crumbs.map((c, i) => (
                                            <span key={c} className="da-crumb-wrap">
                                                {i > 0 && <Icon icon="chevron-right" size={12} className="da-crumb-sep" />}
                                                <button
                                                    className={`da-crumb ${i === crumbs.length - 1 ? "active" : ""}`}
                                                    onClick={() => i < crumbs.length - 1 && navigateTo(c, i)}
                                                    disabled={i === crumbs.length - 1 || !!loadingPath}
                                                >
                                                    {i === 0 ? <Icon icon="database" size={13} style={{ marginRight: 4 }} /> : null}
                                                    {c.split("\\").pop() || c}
                                                </button>
                                            </span>
                                        ))}
                                        {loadingPath && <Spinner size={13} className="da-crumb-spinner" />}
                                    </div>
                                    {selected.size > 0 && (
                                        <Button intent="danger" icon="trash" small text={`Delete ${selected.size}`} onClick={() => setShowBulkDeleteConfirm(true)} disabled={deleting} className="da-bulk-delete-btn" />
                                    )}
                                </div>
                            )}

                            {viewMode === "space" && rows.length > 0 && spaceViewStyle === "treemap" && (
                                <SquarifiedTreemap
                                    items={sortedRows}
                                    totalSize={currentFolderSize}
                                    onDrill={drillInto}
                                    onOpen={openPath}
                                />
                            )}

                            {/* Table */}
                            <div className={`da-table-wrap ${spaceViewStyle === "treemap" ? "with-treemap" : ""}`}>
                                {scanning ? (
                                    <div className="da-center" role="status" aria-live="polite"><PageFlipAnimation size={120} /><div className="da-scan-caption">Searching files & folders<span className="wc-loading-dots">...</span></div></div>
                                ) : viewMode === "large" ? (
                                    largeItems.length === 0 && meta ? (
                                        <div className="da-center"><Icon icon="search-around" size={32} color="var(--text-muted)" /><div className="da-scan-caption">No large files above this threshold</div></div>
                                    ) : largeItems.length === 0 ? (
                                        <div className="da-center da-placeholder"><Icon icon="search-around" size={48} color="var(--text-muted)" /><div className="da-scan-caption">Scan a drive or folder to find large cleanup candidates</div></div>
                                    ) : (
                                        <>
                                            <div className="da-large-filters">
                                                {[
                                                    ["all", "All"],
                                                    ["review", "Review"],
                                                    ["videos", "Videos"],
                                                    ["archives", "Archives"],
                                                    ["installers", "Installers"],
                                                    ["folders", "Folders"],
                                                ].map(([id, label]) => (
                                                    <button
                                                        key={id}
                                                        type="button"
                                                        aria-pressed={largeFilter === id}
                                                        className={largeFilter === id ? "active" : ""}
                                                        onClick={() => setLargeFilter(id as typeof largeFilter)}
                                                    >
                                                        {label}
                                                    </button>
                                                ))}
                                            </div>
                                            <table className="da-table da-large-table">
                                                <thead>
                                                    <tr>
                                                        <th className="da-th-check" scope="col"><span className="sr-only">Select</span></th>
                                                        <th className="da-th-name" scope="col">Name</th>
                                                        <th className="da-th-size" scope="col">Size</th>
                                                        <th className="da-th-items" scope="col">Type</th>
                                                        <th className="da-th-date" scope="col">Modified</th>
                                                        <th className="da-th-actions" scope="col">Actions</th>
                                                    </tr>
                                                </thead>
                                                <tbody>
                                                    {filteredLargeItems.map((row) => {
                                                        const isSelected = selected.has(row.fullPath);
                                                        return (
                                                            <tr key={row.fullPath} className={`da-row da-large-row ${isSelected ? "da-row-selected" : ""}`}>
                                                                <td className="da-td-check" onClick={e => toggleSelect(row.fullPath, e)}>
                                                                    <CheckboxControl checked={isSelected} ariaLabel={`Select ${row.name}`} onChange={() => toggleSelect(row.fullPath)} onClick={e => e.stopPropagation()} />
                                                                </td>
                                                                <td className="da-td-name da-large-name" title={row.fullPath}>
                                                                    <Icon icon={getFileIcon(row.name, row.isDir).icon as any} size={14} className={`da-row-icon ${getFileIcon(row.name, row.isDir).colorClass}`} />
                                                                    <div className="da-large-name-copy">
                                                                        <span className="da-name-text">{row.name}</span>
                                                                        <span className={`da-risk-pill risk-${row.risk}`}>{row.cleanupHint}</span>
                                                                    </div>
                                                                </td>
                                                                <td className="da-td-size">{formatBytes(row.size)}</td>
                                                                <td className="da-td-items">
                                                                    <span className="da-type-pill">{row.itemType}</span>
                                                                </td>
                                                                <td className="da-td-date da-muted">{row.lastModified.slice(0, 16)}</td>
                                                                <td className="da-td-actions">
                                                                    <Button minimal small icon="document-open" title={`Open ${row.name}`} aria-label={`Open ${row.name}`} onClick={() => openPath(row.fullPath)} />
                                                                    <Button minimal small icon="folder-open" title={`Open folder containing ${row.name}`} aria-label={`Open folder containing ${row.name}`} onClick={() => openParentFolder(row.fullPath)} />
                                                                    <Button minimal small icon="trash" intent="danger" title={`Delete ${row.name}`} aria-label={`Delete ${row.name}`} onClick={() => setDeletingSingle(row)} />
                                                                </td>
                                                            </tr>
                                                        );
                                                    })}
                                                </tbody>
                                            </table>
                                        </>
                                    )
                                ) : rows.length === 0 && meta ? (
                                    <div className="da-center"><Icon icon="folder-open" size={32} color="var(--text-muted)" /><div className="da-scan-caption">Empty folder</div></div>
                                ) : rows.length === 0 ? (
                                    <div className="da-center da-placeholder"><Icon icon="search-around" size={48} color="var(--text-muted)" /><div className="da-scan-caption">Enter a path and click Scan</div></div>
                                ) : (
                                    <table className="da-table">
                                        <thead>
                                            <tr>
                                                <th className="da-th-check" scope="col" onClick={toggleSelectAll}>
                                                    <CheckboxControl checked={allSelected} indeterminate={someSelected} ariaLabel="Select all items" onChange={toggleSelectAll} onClick={e => e.stopPropagation()} />
                                                </th>
                                                <th className="da-th-name" scope="col">Name</th>
                                                <th className="da-th-bar" scope="col"><span className="sr-only">Relative size</span></th>
                                                <th className="da-th-size" scope="col">Size</th>
                                                <th className="da-th-alloc" scope="col">Allocated</th>
                                                <th className="da-th-items" scope="col">Items</th>
                                                <th className="da-th-date" scope="col">Modified</th>
                                                <th className="da-th-del" scope="col">Actions</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {rows.map((row) => {
                                                const isLoading = loadingPath === row.fullPath.replace(/\\+$/, "");
                                                const isSelected = selected.has(row.fullPath);
                                                return (
                                                    <tr
                                                        key={row.fullPath}
                                                        className={`da-row ${row.isDir ? "da-row-dir" : "da-row-file"} ${isSelected ? "da-row-selected" : ""}`}
                                                        onClick={() => row.isDir && drillInto(row)}
                                                        title={row.fullPath}
                                                    >
                                                        <td className="da-td-check" onClick={e => toggleSelect(row.fullPath, e)}>
                                                            <CheckboxControl checked={isSelected} ariaLabel={`Select ${row.name}`} onChange={() => toggleSelect(row.fullPath)} onClick={e => e.stopPropagation()} />
                                                        </td>
                                                        <td className="da-td-name">
                                                            <div className="da-td-indent" />
                                                            {isLoading ? (
                                                                <Spinner size={14} className="da-row-spinner" />
                                                            ) : (
                                                                <Icon icon={getFileIcon(row.name, row.isDir).icon as any} size={14} className={`da-row-icon ${getFileIcon(row.name, row.isDir).colorClass}`} />
                                                            )}
                                                            <span className="da-name-text">{row.name}</span>
                                                        </td>
                                                        <td className="da-td-bar"><SizeBar value={row.size} max={totalForBar} /></td>
                                                        <td className="da-td-size">{formatBytes(row.size)}</td>
                                                        <td className="da-td-alloc da-muted">{formatBytes(row.allocated)}</td>
                                                        <td className="da-td-items da-muted">
                                                            {row.isDir ? (
                                                                <div className="da-items-count">
                                                                    {row.fileCount > 0 && <span>{row.fileCount.toLocaleString()} <small>Files</small></span>}
                                                                    {row.folderCount > 0 && <span>{row.folderCount.toLocaleString()} <small>Dirs</small></span>}
                                                                </div>
                                                            ) : ""}
                                                        </td>
                                                        <td className="da-td-date da-muted">{row.lastModified.slice(0, 16)}</td>
                                                        <td className="da-td-del">
                                                            <Button minimal small icon="trash" intent="danger" title={`Delete ${row.name}`} aria-label={`Delete ${row.name}`} onClick={e => { e.stopPropagation(); setDeletingSingle(row); }} />
                                                        </td>
                                                    </tr>

                                                );
                                            })}
                                        </tbody>
                                    </table>
                                )}
                            </div>
                        </div>

                    </div>
                </div>
    );

    return (
        <>
            {inline ? (
                <div className={`disk-analyzer-inline ${Classes.DARK}`}>
                    {innerBody}
                </div>
            ) : (
                <Dialog
                    isOpen={isOpen}
                    onClose={onClose}
                    title="Disk Space Analyzer"
                    icon="search-around"
                    className={`disk-analyzer-dialog ${Classes.DARK}`}
                    style={{ width: "min(95vw, 1280px)", maxHeight: "90vh", display: "flex", flexDirection: "column" }}
                >
                    {innerBody}
                </Dialog>
            )}

            {/* Single delete */}
            <Alert isOpen={!!deletingSingle} cancelButtonText="Cancel" confirmButtonText="Delete" intent="danger" icon="trash"
                onCancel={() => setDeletingSingle(null)} onConfirm={() => deletingSingle && doDelete([deletingSingle.fullPath])} loading={deleting} className={Classes.DARK}>
                <p>Permanently delete <strong>{deletingSingle?.name}</strong>?{deletingSingle?.isDir && " This deletes the entire folder."} This cannot be undone.</p>
            </Alert>

            {/* Bulk delete */}
            <Alert isOpen={showBulkDeleteConfirm} cancelButtonText="Cancel" confirmButtonText={`Delete ${selected.size} items`} intent="danger" icon="trash"
                onCancel={() => setShowBulkDeleteConfirm(false)} onConfirm={() => doDelete(Array.from(selected))} loading={deleting} className={Classes.DARK}>
                <p>Permanently delete <strong>{selected.size} selected item{selected.size > 1 ? "s" : ""}</strong>? This cannot be undone.</p>
            </Alert>
        </>
    );
}
