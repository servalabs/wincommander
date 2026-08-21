import { useCallback, useState } from "react";
import { FileScan, RefreshCw, FileText, FileSpreadsheet, Image, Video, Music, Archive, Code2 } from "lucide-react";
import useBackend, { type StorageStats } from "../../hooks/useBackend";
import SectionCard from "../../components/shared/SectionCard";
import { ui } from "@/assets";
import "./DiskSpaceAnalyzerDialog.css";

interface RowDef { key: keyof StorageStats; label: string; icon: React.ReactNode; }
let fileStatsSession: StorageStats | null = null;

const FILE_ROWS: RowDef[] = [
    { key: "videos",        label: "Videos",        icon: <Video size={12} /> },
    { key: "images",        label: "Images",        icon: <Image size={12} /> },
    { key: "pdfs",          label: "PDFs",          icon: <FileText size={12} /> },
    { key: "documents",     label: "Documents",     icon: <FileText size={12} /> },
    { key: "spreadsheets",  label: "Spreadsheets",  icon: <FileSpreadsheet size={12} /> },
    { key: "presentations", label: "Presentations", icon: <FileSpreadsheet size={12} /> },
    { key: "audio",         label: "Audio",         icon: <Music size={12} /> },
    { key: "archives",      label: "Archives",      icon: <Archive size={12} /> },
    { key: "code",          label: "Code Files",    icon: <Code2 size={12} /> },
];

function fmt(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return String(n);
}
function fmtGb(gb: number): string {
    if (gb === 0) return "0 MB";
    if (gb < 1) return `${(gb * 1024).toFixed(0)} MB`;
    return `${gb.toFixed(1)} GB`;
}

export default function FileStatsPanel() {
    const { getStorageStats } = useBackend();
    const [fileStats, setFileStats] = useState<StorageStats | null>(() => fileStatsSession);
    const [statsScanning, setStatsScanning] = useState(false);
    const [scanError, setScanError] = useState<string | null>(null);

    const runScan = useCallback(async () => {
        setStatsScanning(true);
        setScanError(null);
        try {
            const res = await getStorageStats();
            if (res.success && res.data) {
                fileStatsSession = res.data;
                setFileStats(res.data);
            } else {
                setScanError(res.error || "File statistics could not be collected.");
            }
        } catch (cause) {
            setScanError(String(cause));
        } finally {
            setStatsScanning(false);
        }
    }, [getStorageStats]);

    const maxFileSizeGb = fileStats
        ? Math.max(...FILE_ROWS.map(r => fileStats[r.key]?.sizeGb ?? 0), 0.0001)
        : 0.0001;

    return (
        <SectionCard title="File Statistics" icon="document">
            {/* Reuse the da-col-stats CSS classes from DiskSpaceAnalyzerDialog;
                override width/border so it fills the SectionCard instead of
                sitting as a fixed-width sidebar column. */}
            <div className="da-col-stats" style={{ width: "100%", borderLeft: "none", background: "transparent" }}>
                <div className="da-stats-header">
                    <FileScan size={13} />
                    <span>FILE STATISTICS</span>
                    <button
                        className={`da-stats-scan-btn icon-only${statsScanning ? " scanning" : ""}`}
                        onClick={runScan}
                        disabled={statsScanning}
                        title={fileStats ? "Rescan file statistics" : "Scan file statistics"}
                        aria-label={fileStats ? "Rescan file statistics" : "Scan file statistics"}
                    >
                        {fileStats || statsScanning
                            ? <RefreshCw size={13} className={statsScanning ? "wc-spin" : ""} />
                            : <FileScan size={13} />}
                    </button>
                </div>

                {!fileStats && !statsScanning && !scanError && (
                    <div className="da-stats-empty">
                        <div className="da-empty-icon-wrap">
                            <FileScan size={32} strokeWidth={1.5} />
                        </div>
                        <span className="da-empty-text">
                            Click <strong>SCAN</strong> to analyze<br />file type distribution
                        </span>
                    </div>
                )}

                {statsScanning && (
                    <div className="da-stats-empty scanning" role="status" aria-live="polite">
                        <div className="da-empty-icon-wrap">
                            <img src={ui["searching.gif"]} alt="" className="da-searching-gif" style={{ width: 80, height: 80 }} />
                        </div>
                        <span className="da-empty-text">
                            Analysing files & folders<span className="wc-loading-dots">...</span>
                        </span>
                    </div>
                )}

                {scanError && !statsScanning && (
                    <div className="da-stats-empty" role="alert">
                        <div className="da-empty-icon-wrap"><FileScan size={32} strokeWidth={1.5} /></div>
                        <span className="da-empty-text">File statistics could not be collected.<br />{scanError}</span>
                    </div>
                )}

                {fileStats && !statsScanning && (
                    <div className="da-stats-rows" role="table" aria-label="File type statistics">
                        <div className="sr-only" role="row">
                            <span role="columnheader">File type</span><span role="columnheader">File count</span><span role="columnheader">Storage used</span>
                        </div>
                        {FILE_ROWS.map(({ key, label, icon }) => {
                            const entry = fileStats[key];
                            if (!entry) return null;
                            const barPct = Math.round(((entry.sizeGb ?? 0) / maxFileSizeGb) * 100);
                            return (
                                <div key={key} className="da-stat-row" role="row">
                                    <div className="da-stat-top">
                                        <div className="da-sr-icon" aria-hidden="true">{icon}</div>
                                        <div className="da-sr-label" role="cell">{label}</div>
                                        <div className="da-sr-count" role="cell" aria-label={`${fmt(entry.count)} files`}>{fmt(entry.count)}</div>
                                        <div className="da-sr-size" role="cell">{fmtGb(entry.sizeGb)}</div>
                                    </div>
                                    <div className="da-stat-bottom">
                                        <div className="da-sr-bar-wrap">
                                            <div className="da-sr-bar" style={{ width: `${barPct}%` }} />
                                        </div>
                                    </div>
                                </div>
                            );
                        })}
                    </div>
                )}
            </div>
        </SectionCard>
    );
}
