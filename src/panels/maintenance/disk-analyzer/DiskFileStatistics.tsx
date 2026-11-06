import { useEffect, useState } from "react";
import { Icon, Spinner } from "@/components/ui/bp";
import { ChartNoAxesCombined } from "lucide-react";
import { diskPercent, formatDiskBytes, type DiskFileTypes } from "@/lib/diskAnalyzer";
import useBackend from "@/hooks/useBackend";
import { useFileIconData } from "@/hooks/useFileIconData";
import { getDiskTypeRows, type DiskTypeView } from "./diskFileTypeView";

function AssociatedIcon({ path }: { path?: string | null }) {
    const icon = useFileIconData(path ?? "", false, undefined, !!path);
    return icon ? <img src={icon} alt="" width={18} height={18} /> : <Icon icon="document" size={18} />;
}

export default function DiskFileStatistics({ scanId, path }: { scanId: string; path: string }) {
    const { getDiskFileTypes } = useBackend();
    const [visibleCount, setVisibleCount] = useState(100);
    const [sort, setSort] = useState<"size" | "count" | "name">("size");
    const [view, setView] = useState<DiskTypeView>("category");
    const [state, setState] = useState<{ result: DiskFileTypes | null; error: string; loading: boolean }>({ result: null, error: "", loading: true });
    useEffect(() => {
        let active = true;
        setState({ result: null, error: "", loading: true });
        setVisibleCount(100);
        void getDiskFileTypes(scanId, path).then(result => { if (active) setState({ result, error: "", loading: false }); })
            .catch(error => { if (active) setState({ result: null, error: String(error), loading: false }); });
        return () => { active = false; };
    }, [scanId, path, getDiskFileTypes]);
    const rows = getDiskTypeRows(state.result, view, sort);
    const total = rows.reduce((sum, row) => sum + row.logicalSize, 0);
    return <section className="da-file-statistics" aria-label="File statistics for current folder" aria-busy={state.loading}>
        <header><ChartNoAxesCombined size={19} /><h3>Files by type</h3>
        <div className="da-file-pane-tabs" role="group" aria-label="File type breakdown">
            <button type="button" aria-pressed={view === "category"} onClick={() => { setView("category"); setVisibleCount(100); }}>Categories</button>
            <button type="button" aria-pressed={view === "extension"} onClick={() => { setView("extension"); setVisibleCount(100); }}>Extensions</button>
        </div>
        {!state.loading && !state.error && state.result?.complete === false && <span className="da-type-warning" role="status" tabIndex={0} title="Partial scan: these totals cover only files that were scanned." aria-label="Partial scan: these totals cover only files that were scanned."><Icon icon="warning-sign" size={16} /></span>}</header>
        {state.loading ? <div className="da-chart-empty" role="status"><Spinner size={22} />Reading scanned file types…</div> : state.error ? <p role="alert">{state.error}</p> : view === "extension" && !Array.isArray(state.result?.extensions) ? <p role="status">Extension details are unavailable for this scan. Update WinCommander and analyze again.</p> : <>
            <label className="da-type-sort">Sort by <select value={sort} onChange={event => setSort(event.target.value as "size" | "count" | "name")}><option value="size">Largest size</option><option value="count">File count</option><option value="name">Name</option></select></label>
            <div className="da-type-table" role="table" aria-label={`Scanned files by ${view}`}><div role="row" className="da-type-row da-type-head"><span role="columnheader">{view === "category" ? "Category" : "Extension"}</span><span role="columnheader">Files</span><span role="columnheader">Size</span><span role="columnheader">Share</span></div>
                {rows.slice(0, visibleCount).map(row => <div role="row" className="da-type-row" key={row.category}><span role="cell" className="da-type-name"><AssociatedIcon path={row.samplePath} /> {row.category}<span className="da-type-meter"><span style={{ width: `${diskPercent(row.logicalSize, total)}%` }} /></span></span><span role="cell">{row.fileCount.toLocaleString()}</span><span role="cell">{formatDiskBytes(row.logicalSize)}</span><span role="cell">{diskPercent(row.logicalSize, total).toFixed(1)}%</span></div>)}
                <div role="row" className="da-type-row da-type-total"><strong role="cell">Total</strong><strong role="cell">{rows.reduce((sum, row) => sum + row.fileCount, 0).toLocaleString()}</strong><strong role="cell">{formatDiskBytes(total)}</strong><span role="cell">{total ? "100%" : "0%"}</span></div>
            </div>{rows.length > visibleCount && <button type="button" onClick={() => setVisibleCount(count => count + 100)}>Show more {view === "category" ? "categories" : "extensions"} ({rows.length - visibleCount} remaining)</button>}</>}
    </section>;
}
