import { useState, useCallback } from "react";
import { FileText, FileSpreadsheet, Image, Video, Music, Archive, Code2, FileScan, RefreshCw } from "lucide-react";
import useBackend, { type StorageStats } from "../../hooks/useBackend";
import "./StorageStatsCard.css";

// ── helpers ──────────────────────────────────────────────────────────────────

function fmt(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000)     return `${(n / 1_000).toFixed(1)}K`;
    return String(n);
}

function fmtGb(gb: number): string {
    if (gb === 0) return "0 MB";
    if (gb < 0.01) return `${(gb * 1024).toFixed(0)} MB`;
    if (gb < 1)    return `${(gb * 1024).toFixed(0)} MB`;
    return `${gb.toFixed(1)} GB`;
}

// ── row config ────────────────────────────────────────────────────────────────

interface RowDef {
    key: keyof StorageStats;
    label: string;
    icon: React.ReactNode;
    color: string;
}

const ROWS: RowDef[] = [
    { key: "videos",       label: "Videos",        icon: <Video size={12} />,          color: "var(--color-accent)" },
    { key: "images",       label: "Images",         icon: <Image size={12} />,          color: "var(--color-accent)" },
    { key: "pdfs",         label: "PDFs",           icon: <FileText size={12} />,       color: "var(--color-accent)" },
    { key: "documents",    label: "Documents",      icon: <FileText size={12} />,       color: "var(--color-accent)" },
    { key: "spreadsheets", label: "Spreadsheets",   icon: <FileSpreadsheet size={12} />, color: "var(--color-accent)" },
    { key: "presentations",label: "Presentations",  icon: <FileSpreadsheet size={12} />, color: "var(--color-accent)" },
    { key: "audio",        label: "Audio",          icon: <Music size={12} />,          color: "var(--color-accent)" },
    { key: "archives",     label: "Archives",       icon: <Archive size={12} />,        color: "var(--color-accent)" },
    { key: "code",         label: "Code Files",     icon: <Code2 size={12} />,          color: "var(--color-accent)" },
];

// ── component ─────────────────────────────────────────────────────────────────

export default function StorageStatsCard() {
    const [stats, setStats] = useState<StorageStats | null>(null);
    const [scanning, setScanning] = useState(false);
    const [source, setSource] = useState<string | null>(null);
    const { getStorageStats } = useBackend();

    const runScan = useCallback(async () => {
        setScanning(true);
        try {
            const res = await getStorageStats();
            if (res.success && res.data) {
                setStats(res.data);
                // Detect source from first entry that has it
                const firstEntry = Object.values(res.data)[0] as any;
                if (firstEntry?.source) setSource(firstEntry.source);
            }
        } finally {
            setScanning(false);
        }
    }, [getStorageStats]);

    // Compute max count for bar scaling
    const maxCount = stats
        ? Math.max(...ROWS.map(r => stats[r.key]?.count ?? 0), 1)
        : 1;

    return (
        <div className="storage-stats-section">
            <div className="card-header">
                <div className="header-icon-group" style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <FileScan size={14} />
                    <span>FILE STATISTICS</span>
                </div>
                <button
                    className={`storage-scan-btn${scanning ? ' scanning' : ''}`}
                    onClick={runScan}
                    disabled={scanning}
                    title="Scan all drives"
                >
                    <RefreshCw size={11} className={scanning ? 'spin' : ''} />
                    <span>{scanning ? 'SCANNING...' : (stats ? 'RESCAN' : 'SCAN')}</span>
                </button>
            </div>

            {!stats && !scanning && (
                <div className="storage-stats-empty">
                    <FileScan size={20} style={{ opacity: 0.3 }} />
                    <span>Click SCAN to index file types across all drives</span>
                    {source !== 'everything' && (
                        <span className="storage-stats-hint">Install Instant Search Engine</span>
                    )}
                </div>
            )}

            {scanning && (
                <div className="storage-stats-empty">
                    <FileScan size={20} className="spin" style={{ opacity: 0.5 }} />
                    <span>Scanning all drives...</span>
                </div>
            )}

            {stats && !scanning && (
                <div className="storage-stats-rows">
                    {ROWS.map(({ key, label, icon }) => {
                        const entry = stats[key];
                        if (!entry) return null;
                        const barPct = maxCount > 0 ? Math.round((entry.count / maxCount) * 100) : 0;
                        return (
                            <div key={key} className="storage-stat-row">
                                <div className="ss-icon">{icon}</div>
                                <div className="ss-label">{label}</div>
                                <div className="ss-bar-wrap">
                                    <div
                                        className="ss-bar"
                                        style={{ width: `${barPct}%` }}
                                    />
                                </div>
                                <div className="ss-count">{fmt(entry.count)}</div>
                                <div className="ss-size">{fmtGb(entry.sizeGb)}</div>
                            </div>
                        );
                    })}
                </div>
            )}
        </div>
    );
}
