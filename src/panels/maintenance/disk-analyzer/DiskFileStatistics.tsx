import { useEffect, useId, useState } from "react";
import { Icon, Spinner } from "@/components/ui/bp";
import { diskPercent, formatDiskBytes, type DiskFileTypes } from "@/lib/diskAnalyzer";
import useBackend from "@/hooks/useBackend";
import { useFileIconData } from "@/hooks/useFileIconData";
import { getDiskTypeRows, type DiskTypeView } from "./diskFileTypeView";

function AssociatedIcon({ path }: { path?: string | null }) {
    const icon = useFileIconData(path ?? "", false, undefined, !!path);
    return <span aria-hidden="true">{icon ? <img src={icon} alt="" width={18} height={18} /> : <Icon icon="document" size={18} />}</span>;
}

export default function DiskFileStatistics({ scanId, path }: { scanId: string; path: string }) {
    const { getDiskFileTypes } = useBackend();
    const sortId = useId();
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
        <header><Icon icon="chart" size={21} /><div><h3>Files by type</h3><p>Counts and logical sizes from the same scan, including this folder’s descendants.</p></div></header>
        {state.loading ? <div className="da-chart-empty"><Spinner size={22} />Reading scanned file types…</div> : state.error ? <p role="alert">{state.error}</p> : <>
            {state.result?.complete !== true && <p className="da-scan-notice" role="status">Partial scan: these totals cover only files that were scanned.</p>}
            <div className="da-type-view" role="group" aria-label="File type breakdown">
                <button type="button" aria-pressed={view === "category"} onClick={() => { setView("category"); setVisibleCount(100); }}>Categories</button>
                <button type="button" aria-pressed={view === "extension"} onClick={() => { setView("extension"); setVisibleCount(100); }}>Extensions</button>
            </div>
            {view === "extension" && !Array.isArray(state.result?.extensions) ? <p role="status">Extension details are unavailable for this scan. Update WinCommander and analyze again.</p> : <>
                <div className="da-type-sort"><label htmlFor={sortId}>Sort by</label><select id={sortId} value={sort} onChange={event => setSort(event.target.value as "size" | "count" | "name")}><option value="size">Largest size</option><option value="count">File count</option><option value="name">Name</option></select></div>
                <div className="da-type-scroll" role="region" aria-label={`Scanned files by ${view}`} tabIndex={0}>
                    <table className="da-type-table"><caption>Scanned files by {view}</caption><thead><tr className="da-type-row da-type-head"><th scope="col">{view === "category" ? "Category" : "Extension"}</th><th scope="col">Files</th><th scope="col">Logical size</th><th scope="col">Share</th></tr></thead>
                        <tbody>{rows.slice(0, visibleCount).map(row => <tr className="da-type-row" key={row.category}><th scope="row" className="da-type-name"><AssociatedIcon path={row.samplePath} /> {row.category}<span className="da-type-meter" aria-hidden="true"><span style={{ width: `${diskPercent(row.logicalSize, total)}%` }} /></span></th><td>{row.fileCount.toLocaleString()}</td><td>{formatDiskBytes(row.logicalSize)}</td><td>{diskPercent(row.logicalSize, total).toFixed(1)}%</td></tr>)}</tbody>
                        <tfoot><tr className="da-type-row da-type-total"><th scope="row">Total</th><td>{rows.reduce((sum, row) => sum + row.fileCount, 0).toLocaleString()}</td><td>{formatDiskBytes(total)}</td><td>{total ? "100%" : "0%"}</td></tr></tfoot>
                    </table>
                </div>
                {rows.length > visibleCount && <button type="button" onClick={() => setVisibleCount(count => count + 100)}>Show more {view === "category" ? "categories" : "extensions"} ({rows.length - visibleCount} remaining)</button>}
            </>}
        </>}
    </section>;
}
