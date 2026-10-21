import { open as openFolderDialog } from "@tauri-apps/plugin-dialog";
import { Button, Icon, Spinner } from "@/components/ui/bp";
import { diskBreadcrumbs, diskScopeTotals, formatDiskBytes, normalizeDiskPath } from "@/lib/diskAnalyzer";
import { showError } from "@/utils/toast";
import type { DiskAnalyzer } from "./useDiskAnalyzer";

export default function DiskAnalyzerHeader({ analyzer, mode }: { analyzer: DiskAnalyzer; mode: "space" | "large" }) {
    const { scanning, meta, stale, deleting, loadingPath, history, historyIndex, currentPath } = analyzer;
    const busy = scanning || deleting || loadingPath;
    const crumbs = meta ? diskBreadcrumbs(meta.scanRoot, currentPath) : [];
    const scope = mode === "space" ? diskScopeTotals(analyzer.rows) : { size: meta?.totalSize ?? 0, files: meta?.fileCount ?? 0, folders: meta?.folderCount ?? 0 };
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
                {mode === "large" && <label className="da-minimum-size">Minimum size<select aria-label="Minimum file size" value={analyzer.minSize} disabled={busy || analyzer.largeLoading} onChange={event => void analyzer.changeMinSize(Number(event.target.value))}>{[16, 50, 100, 500, 1024, 5120].map(size => <option key={size} value={size * 1024 * 1024}>{size >= 1024 ? `${size / 1024} GiB` : `${size} MiB`}</option>)}</select></label>}
                <Button intent="primary" icon={meta ? "refresh" : "search"} disabled={busy || !analyzer.scanPath.trim()} onClick={() => void analyzer.startScan()}>{meta ? "Analyze again" : "Analyze"}</Button>
                {scanning && <Button intent="danger" minimal disabled={analyzer.cancelling} onClick={() => void analyzer.cancelScan()}>{analyzer.cancelling ? "Stopping…" : "Stop scan"}</Button>}
            </div>
        </header>
        <div className="da-scope-caption"><span>Sizes are logical file sizes · Allocated disk size is unavailable.</span>
            {meta && !stale && <span title={mode === "space" ? currentPath : meta.scanRoot}>{mode === "space" ? "Current folder" : "Searched location"}: {mode === "space" ? currentPath : meta.scanRoot}</span>}
        </div>
        {meta && !stale && normalizeDiskPath(analyzer.scanPath) !== normalizeDiskPath(meta.scanRoot) && <div className="da-scan-notice" role="status"><Icon icon="info-sign" size={15} /><span>Showing the last analyzed location above. Analyze the newly chosen location to update these results.</span></div>}
        {meta && !stale && !scanning && <div className="da-metrics" aria-label="Scan summary">
            <div><small>{mode === "space" ? "Current folder content" : "Scanned content"}</small><strong>{formatDiskBytes(scope.size)}</strong></div>
            <div><small>Files</small><strong>{scope.files.toLocaleString()}</strong></div>
            <div><small>Folders</small><strong>{scope.folders.toLocaleString()}</strong></div>
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
