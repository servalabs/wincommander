import { useEffect, useMemo, useState } from "react";
import { Alert, Button, Dialog, Icon, Spinner } from "@/components/ui/bp";
import { filterLargeDiskItems, formatDiskBytes, type DiskAnalyzerMode, type DiskSort, type LargeDiskFilter } from "@/lib/diskAnalyzer";
import DiskAnalyzerTabs from "./disk-analyzer/DiskAnalyzerTabs";
import DiskFileStatistics from "./disk-analyzer/DiskFileStatistics";
import DiskAnalyzerHeader from "./disk-analyzer/DiskAnalyzerHeader";
import DiskCharts from "./disk-analyzer/DiskCharts";
import DiskItemDetails from "./disk-analyzer/DiskItemDetails";
import DiskItemList from "./disk-analyzer/DiskItemList";
import { useDiskAnalyzer } from "./disk-analyzer/useDiskAnalyzer";
import { useDiskChart } from "./disk-analyzer/useDiskChart";
import "./DiskSpaceAnalyzerDialog.css";

interface Props {
    isOpen: boolean;
    onClose: () => void;
    initialMode?: DiskAnalyzerMode;
    inline?: boolean;
}

const LARGE_FILTERS: { id: LargeDiskFilter; label: string }[] = [
    { id: "all", label: "All files" }, { id: "review", label: "Higher risk" }, { id: "videos", label: "Videos" },
    { id: "archives", label: "Archives" }, { id: "installers", label: "Installers" }, { id: "documents", label: "Documents" }, { id: "images", label: "Images" }, { id: "audio", label: "Audio" },
];

export default function DiskSpaceAnalyzerDialog({ isOpen, onClose, initialMode = "space", inline = false }: Props) {
    const analyzer = useDiskAnalyzer();
    const [mode, setMode] = useState<DiskAnalyzerMode>(initialMode);
    const [chartMode, setChartMode] = useState<"treemap" | "sunburst">("treemap");
    const [depth, setDepth] = useState(2);
    const [filter, setFilter] = useState<LargeDiskFilter>("all");
    const [search, setSearch] = useState("");
    const [sort, setSort] = useState<DiskSort>("size");
    const busy = analyzer.scanning || analyzer.deleting || analyzer.loadingPath || analyzer.largeLoading;
    const chart = useDiskChart(analyzer.rows, depth, analyzer.getChildren, analyzer.scanning || analyzer.stale || mode !== "space" || !inline && !isOpen);
    const { setSelected, setFocused } = analyzer;
    useEffect(() => { setMode(initialMode); }, [initialMode]);
    useEffect(() => { setSelected(new Set()); setFocused(null); }, [mode, setSelected, setFocused]);
    const rows = useMemo(() => [...analyzer.rows].sort((a, b) => b.size - a.size), [analyzer.rows]);
    const largeItems = useMemo(() => filterLargeDiskItems(analyzer.largeItems, filter, search, sort), [analyzer.largeItems, filter, search, sort]);
    const items = mode === "space" ? rows : largeItems;
    const selectedItems = analyzer.selectedItems();
    const selectedBytes = selectedItems.reduce((sum, item) => sum + item.size, 0);
    const totalSize = mode === "space" ? rows.reduce((sum, item) => sum + item.size, 0) : analyzer.meta?.totalSize ?? 0;
    const selectableItems = items.filter(item => !item.isDir);
    const allMatchingSelected = selectableItems.length > 0 && selectableItems.every(item => analyzer.selected.has(item.fullPath));

    function selectMatchingFiles() {
        analyzer.setSelected(previous => {
            const next = new Set(previous);
            selectableItems.forEach(item => { if (allMatchingSelected) next.delete(item.fullPath); else next.add(item.fullPath); });
            return next;
        });
    }
    const content = <div className="da-workspace">
        <DiskAnalyzerHeader analyzer={analyzer} mode={mode}><DiskAnalyzerTabs mode={mode} disabled={analyzer.deleting} onChange={setMode} /></DiskAnalyzerHeader>
        <div id="disk-analyzer-view" role="tabpanel" aria-labelledby={`disk-tab-${mode}`} className="da-analyzer-content">
        {analyzer.scanning ? <div className="da-workspace-empty" role="status"><Spinner size={38} /><h3>Analyzing your storage</h3>
            <p>{analyzer.progress ? `${analyzer.progress.filesScanned.toLocaleString()} files · ${analyzer.progress.foldersScanned.toLocaleString()} folders · ${formatDiskBytes(analyzer.progress.bytesScanned)} found` : "Reading files and folders. Scan time depends on the selected location."}</p>
            <small>{analyzer.cancelling ? "Stopping safely. Partial results will remain available." : "You can leave this workspace while the scan continues."}</small>
        </div> : analyzer.stale ? <div className="da-workspace-empty" role="status"><Icon icon="refresh" size={42} /><h3>Refresh your storage view</h3><p>Content was removed. Analyze again for current sizes and folder totals.</p><Button intent="primary" disabled={busy} onClick={() => void analyzer.startScan()}>Analyze again</Button></div>
            : !analyzer.meta ? <div className="da-workspace-empty"><span className="da-empty-disc"><Icon icon="database" size={40} /></span><h3>{mode === "large" ? "Find files above your chosen size" : "See where your space goes"}</h3><p>{mode === "large" ? "Choose a drive or folder and a minimum size, then analyze it. Every matching scanned file is counted; any result limit is shown." : "Choose a drive or folder above, then analyze it to explore a visual breakdown and find large files."}</p><div className="da-empty-features"><span><Icon icon="heatmap" size={15} />Interactive treemap</span><span><Icon icon="pie-chart" size={15} />Sunburst view</span><span><Icon icon="folder-open" size={15} />Folder drill-down</span></div></div>
                : mode === "types" ? <DiskFileStatistics scanId={analyzer.meta.scanId} path={analyzer.currentPath} /> : <>
                    {mode === "large" && <div className="da-large-options">
                        <div className="da-filter-buttons" aria-label="Large file type">{LARGE_FILTERS.map(item => <button type="button" key={item.id} disabled={busy} aria-pressed={filter === item.id} className={filter === item.id ? "active" : ""} onClick={() => setFilter(item.id)}>{item.label}</button>)}</div>
                        <div className="da-large-inputs"><label><span className="sr-only">Search file name or path</span><input type="search" value={search} onChange={event => setSearch(event.target.value)} placeholder="Search name or path…" /></label>
                            <label>Sort<select value={sort} disabled={busy} onChange={event => setSort(event.target.value as DiskSort)}><option value="size">Largest first</option><option value="name">Name</option><option value="modified">Recently modified</option></select></label></div>
                        <p role="status">{analyzer.largeResult ? <>{analyzer.largeResult.matchedCount.toLocaleString()} files ≥ {formatDiskBytes(analyzer.largeResult.minimumSize)} · {formatDiskBytes(analyzer.largeResult.matchedSize)} logical size in the scanned location.
                            {analyzer.largeResult.limited ? ` Showing the largest ${analyzer.largeResult.resultLimit.toLocaleString()} matches; search, type filters and sorting apply to these loaded results only. Increase the minimum size or scan a smaller folder to inspect more matches.` : " All threshold matches from the scan are loaded."}
                            {analyzer.meta.complete === false ? " Scan is partial; unscanned files are not included." : ""}</> : "Loading threshold matches…"}</p>
                    </div>}
                    <div className={`da-explorer${mode === "large" ? " da-explorer-large" : ""}`} aria-busy={busy}>
                        <section className="da-folders" aria-label={mode === "space" ? "Folder breakdown" : "Large file results"}>
                            <div className="da-pane-toolbar"><span>{items.length.toLocaleString()} {mode === "large" ? "matching files shown" : "items"}{mode === "space" ? ` · ${formatDiskBytes(totalSize)}` : ""}</span><button type="button" disabled={busy || !selectableItems.length} onClick={selectMatchingFiles}>{allMatchingSelected ? "Deselect" : "Select"} all {selectableItems.length.toLocaleString()} matching files</button></div>
                            {analyzer.largeLoading && mode === "large" ? <div className="da-chart-empty" role="status"><Spinner size={22} />Finding threshold matches…</div> : <DiskItemList items={items} total={totalSize} selected={analyzer.selected} focused={analyzer.focused} large={mode === "large"} disabled={busy} onFocus={analyzer.setFocused} onSelect={analyzer.toggleSelection} onNavigate={path => void analyzer.navigate(path)} />}
                            <DiskItemDetails item={analyzer.focused} disabled={busy} onOpen={item => void analyzer.openInExplorer(item)} onNavigate={path => void analyzer.navigate(path)} onDelete={item => analyzer.setConfirmItems([item])} />
                        </section>
                        {mode === "space" && <section className="da-visualization" aria-label="Storage visualization"><div className="da-pane-toolbar"><span>{formatDiskBytes(totalSize)} in this folder</span><div className="da-chart-controls">
                            <label><span className="sr-only">Chart depth</span><select value={depth} disabled={busy} onChange={event => setDepth(Number(event.target.value))}>{[1, 2, 3].map(value => <option key={value} value={value}>Depth {value}</option>)}</select></label>
                            <div className="da-chart-toggle"><button type="button" disabled={busy} aria-pressed={chartMode === "treemap"} className={chartMode === "treemap" ? "active" : ""} onClick={() => setChartMode("treemap")}><Icon icon="heatmap" size={13} />Treemap</button><button type="button" disabled={busy} aria-pressed={chartMode === "sunburst"} className={chartMode === "sunburst" ? "active" : ""} onClick={() => setChartMode("sunburst")}><Icon icon="pie-chart" size={13} />Sunburst</button></div>
                        </div></div>
                            <div className="da-chart-wrap"><DiskCharts scopePath={analyzer.currentPath} nodes={chart.nodes} depth={depth} mode={chartMode} focused={analyzer.focused} onFocus={analyzer.setFocused} onNavigate={path => void analyzer.navigate(path)} disabled={busy} /></div>
                            <div className="da-chart-caption" role="status">{chart.loading ? <><Spinner size={13} />Loading folder depth…</> : chart.limited ? "Display limit: folder depth loads at most 60 folders. Other retains collapsed counts and sizes; open a folder for detail." : "Select for details · Double-click a folder to explore"}</div>
                        </section>}
                    </div>
                </>}
        </div>
        <footer className="da-selection-bar"><div><Icon icon="layers" size={17} /><span><strong>{selectedItems.length} selected</strong><small>{formatDiskBytes(selectedBytes)} logical size · Space recovered may differ</small></span></div><div>
            {analyzer.selected.size > 0 && <Button minimal disabled={busy} onClick={() => analyzer.setSelected(new Set())}>Clear</Button>}
            <Button intent="danger" icon="trash" disabled={busy || !selectedItems.length || analyzer.stale} onClick={() => analyzer.setConfirmItems(selectedItems)}>Review deletion</Button>
        </div></footer>
    </div>;
    return <>
        {inline ? <div className="disk-analyzer-inline">{content}</div> : <Dialog isOpen={isOpen} onClose={onClose} title="Disk Space Analyzer" icon="search-around" className="disk-analyzer-dialog">{content}</Dialog>}
        <Alert isOpen={analyzer.confirmItems.length > 0} cancelButtonText="Cancel" confirmButtonText={`Delete ${analyzer.confirmItems.length} item${analyzer.confirmItems.length === 1 ? "" : "s"}`} intent="danger" icon="trash" loading={analyzer.deleting} onCancel={() => { if (!analyzer.deleting) analyzer.setConfirmItems([]); }} onConfirm={() => void analyzer.deleteConfirmed()}>
            <p>Permanently delete <strong>{analyzer.confirmItems.length} file{analyzer.confirmItems.length === 1 ? "" : "s"}</strong> ({formatDiskBytes(analyzer.confirmItems.reduce((sum, item) => sum + item.size, 0))} logical size)? This cannot be undone.</p>
            <ul className="da-confirm-list">{analyzer.confirmItems.map(item => <li key={item.fullPath}>{item.fullPath}</li>)}</ul>
        </Alert>
    </>;
}
