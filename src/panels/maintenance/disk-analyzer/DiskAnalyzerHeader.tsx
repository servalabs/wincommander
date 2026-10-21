import { open as openFolderDialog } from "@tauri-apps/plugin-dialog";
import { Button, Icon, Spinner } from "@/components/ui/bp";
import { diskBreadcrumbs, formatDiskBytes } from "@/lib/diskAnalyzer";
import { showError } from "@/utils/toast";
import type { DiskAnalyzer } from "./useDiskAnalyzer";

export default function DiskAnalyzerHeader({ analyzer, mode, onMode }: { analyzer: DiskAnalyzer; mode: "space" | "large"; onMode: (mode: "space" | "large") => void }) {
    const { scanning, meta, stale, deleting, loadingPath, history, historyIndex, currentPath } = analyzer;
    const busy = scanning || deleting || loadingPath;
    const crumbs = meta ? diskBreadcrumbs(meta.scanRoot, currentPath) : [];
    async function browse() {
        try {
            const selected = await openFolderDialog({ directory: true, multiple: false, title: "Choose a drive or folder to analyze" });
            if (typeof selected === "string") analyzer.setScanPath(selected);
        } catch (error) { showError(String(error)); }
    }
    return <>
        <header className="da-workspace-header">
            <div className="da-heading"><span className="da-eyebrow">STORAGE EXPLORER</span><h2>{mode === "space" ? "Disk Space Analysis" : "Large Files"}</h2><p>Understand what takes up space. Review before removing.</p></div>
            <div className="da-scan-controls"><div className="da-location-input"><Icon icon="folder-open" size={15} />
                <input aria-label="Drive or folder to analyze" value={analyzer.scanPath} disabled={busy} spellCheck={false}
                    onChange={event => analyzer.setScanPath(event.target.value)} onKeyDown={event => { if (event.key === "Enter") void analyzer.startScan(); }} />
                <Button minimal small icon="more" title="Choose folder" aria-label="Choose folder" disabled={busy} onClick={browse} />
            </div>
                <Button intent="primary" icon={meta ? "refresh" : "search"} disabled={busy || !analyzer.scanPath.trim()} loading={scanning} onClick={() => void analyzer.startScan()}>{meta ? "Analyze again" : "Analyze"}</Button>
                {scanning && <Button intent="danger" minimal disabled={analyzer.cancelling} onClick={() => void analyzer.cancelScan()}>{analyzer.cancelling ? "Stopping…" : "Stop scan"}</Button>}
            </div>
        </header>
        <div className="da-workspace-tabs" aria-label="Storage view">
            <button type="button" aria-pressed={mode === "space"} className={mode === "space" ? "active" : ""} disabled={deleting} onClick={() => onMode("space")}><Icon icon="heatmap" size={15} />Disk usage</button>
            <button type="button" aria-pressed={mode === "large"} className={mode === "large" ? "active" : ""} disabled={deleting} onClick={() => onMode("large")}><Icon icon="document" size={15} />Large files</button>
            <span>Sizes are logical file sizes</span>
        </div>
        {meta && !stale && <div className="da-metrics" aria-label="Scan summary">
            <div><small>Scanned content</small><strong>{formatDiskBytes(meta.totalSize)}</strong></div>
            <div><small>Files</small><strong>{meta.fileCount.toLocaleString()}</strong></div>
            <div><small>Folders</small><strong>{meta.folderCount.toLocaleString()}</strong></div>
            <div><small>Volume free space</small><strong>{meta.driveCapacity > 0 ? formatDiskBytes(meta.freeSpace) : "Unavailable"}</strong></div>
            <div><small>Volume capacity</small><strong>{meta.driveCapacity > 0 ? formatDiskBytes(meta.driveCapacity) : "Unavailable"}</strong></div>
        </div>}
        {meta && !stale && (meta.complete === false || meta.skippedEntries || meta.skippedLinks || meta.cancelled || meta.truncated) ? <div className="da-scan-notice" role="status"><Icon icon="warning-sign" size={15} />
            <span>{meta.cancelled ? "Scan stopped. " : "Partial scan. "}{meta.skippedEntries ? `${meta.skippedEntries.toLocaleString()} entries skipped${meta.skippedLinks ? `, including ${meta.skippedLinks.toLocaleString()} links` : ""}. ` : ""}{meta.truncated ? "Scan limit reached. " : ""}Totals cover scanned content only.</span>
        </div> : null}
        {mode === "space" && meta && !stale && <nav className="da-location-nav" aria-label="Folder navigation">
            <Button minimal small icon="chevron-left" title="Back" aria-label="Back" disabled={busy || historyIndex <= 0} onClick={() => void analyzer.navigate(history[historyIndex - 1], historyIndex - 1)} />
            <Button minimal small icon="chevron-right" title="Forward" aria-label="Forward" disabled={busy || historyIndex >= history.length - 1} onClick={() => void analyzer.navigate(history[historyIndex + 1], historyIndex + 1)} />
            <Button minimal small icon="arrow-up" title="Parent folder" aria-label="Parent folder" disabled={busy || crumbs.length < 2} onClick={() => void analyzer.navigate(crumbs[crumbs.length - 2])} />
            <div className="da-crumb-list">{crumbs.map((path, index) => <span key={path}>
                {index > 0 && <Icon icon="chevron-right" size={11} />}<button type="button" aria-current={index === crumbs.length - 1 ? "location" : undefined} disabled={busy || index === crumbs.length - 1} onClick={() => void analyzer.navigate(path)} title={path}>{index === 0 ? path : path.split("\\").pop()}</button>
            </span>)}</div>{loadingPath && <Spinner size={16} />}
        </nav>}
    </>;
}
