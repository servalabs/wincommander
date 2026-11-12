import { useEffect, useState } from "react";
import { Spinner } from "@/components/ui/bp";
import { ChartNoAxesCombined } from "lucide-react";
import { diskPercent, formatDiskBytes, type DiskFileTypes } from "@/lib/diskAnalyzer";
import useBackend from "@/hooks/useBackend";

export default function DiskFileStatistics({ scanId, path }: { scanId: string; path: string }) {
    const { getDiskFileTypes } = useBackend();
    const [visibleCount, setVisibleCount] = useState(100);
    const [sort, setSort] = useState<"size" | "count" | "name">("size");
    const [state, setState] = useState<{ result: DiskFileTypes | null; error: string; loading: boolean }>({ result: null, error: "", loading: true });
    useEffect(() => {
        let active = true;
        setState({ result: null, error: "", loading: true });
        setVisibleCount(100);
        void getDiskFileTypes(scanId, path).then(result => { if (active) setState({ result, error: "", loading: false }); })
            .catch(error => { if (active) setState({ result: null, error: String(error), loading: false }); });
        return () => { active = false; };
    }, [scanId, path, getDiskFileTypes]);
    const rows = [...(state.result?.extensions ?? [])].sort((a, b) => sort === "name" ? a.category.localeCompare(b.category) : sort === "count" ? b.fileCount - a.fileCount || a.category.localeCompare(b.category) : b.logicalSize - a.logicalSize || a.category.localeCompare(b.category));
    const total = rows.reduce((sum, row) => sum + row.logicalSize, 0);
    return <section className="da-file-statistics" aria-label="File statistics for current folder" aria-busy={state.loading}>
        <header><ChartNoAxesCombined size={19} /><div><h3>Files by extension</h3><p>Counts and logical sizes include this folder and its descendants.</p></div></header>
        {state.loading ? <div className="da-chart-empty" role="status"><Spinner size={22} />Reading scanned file types…</div> : state.error ? <p role="alert">{state.error}</p> : !Array.isArray(state.result?.extensions) ? <p role="status">Extension details are unavailable for this scan. Update WinCommander and analyze again.</p> : <>
            {!state.result?.complete && <p className="da-scan-notice" role="status">Partial scan: these totals cover only files that were scanned.</p>}
            <label className="da-type-sort">Sort by <select value={sort} onChange={event => setSort(event.target.value as "size" | "count" | "name")}><option value="size">Largest size</option><option value="count">File count</option><option value="name">Extension</option></select></label>
            <div className="da-type-table" role="table" aria-label="Scanned files by type"><div role="row" className="da-type-row da-type-head"><span role="columnheader">Extension</span><span role="columnheader">Files</span><span role="columnheader">Logical size</span><span role="columnheader">Share</span></div>
                {rows.slice(0, visibleCount).map(row => <div role="row" className="da-type-row" key={row.category}><span role="cell" className="da-type-name">{row.category}<span className="da-type-meter"><span style={{ width: `${diskPercent(row.logicalSize, total)}%` }} /></span></span><span role="cell">{row.fileCount.toLocaleString()}</span><span role="cell">{formatDiskBytes(row.logicalSize)}</span><span role="cell">{diskPercent(row.logicalSize, total).toFixed(1)}%</span></div>)}
                <div role="row" className="da-type-row da-type-total"><strong role="cell">Total</strong><strong role="cell">{rows.reduce((sum, row) => sum + row.fileCount, 0).toLocaleString()}</strong><strong role="cell">{formatDiskBytes(total)}</strong><span role="cell">{total ? "100%" : "0%"}</span></div>
            </div>{rows.length > visibleCount && <button type="button" onClick={() => setVisibleCount(count => count + 100)}>Show more extensions ({rows.length - visibleCount} remaining)</button>}</>}
    </section>;
}
