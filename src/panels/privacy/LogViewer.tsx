import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/bp";
import { Icon } from "../../components/ui/icon";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";
import { showSuccess, showError } from "../../utils/toast";
import {
    filterLogRecords,
    groupLogRecords,
    dedupNearSimultaneous,
    levelFilterToBackendLevels,
    LEVEL_FILTERS,
    type LogRecord,
    type LevelFilter,
    type SourceFilter,
} from "../../lib/logFilter";
import "./LogViewer.css";

const SOURCES: SourceFilter[] = ["ALL", "UI", "CORE", "PRO"];

const LEVEL_COLOR: Record<LevelFilter, string> = {
    ALL: "var(--color-text-muted)",
    ERROR_WARN: "var(--color-warning, #f59e0b)",
    ERROR: "var(--color-danger, #ef4444)",
    WARN:  "var(--color-warning, #f59e0b)",
    INFO:  "var(--color-info, #3b82f6)",
};

const LEVEL_LABEL: Record<LevelFilter, string> = {
    ALL: "ALL",
    ERROR_WARN: "ERROR+WARN",
    ERROR: "ERROR",
    WARN: "WARN",
    INFO: "INFO",
};

const colorForRecordLevel = (level: string) => {
    const normalized = level.toUpperCase();
    if (normalized === "ERROR" || normalized === "WARN" || normalized === "INFO") {
        return LEVEL_COLOR[normalized];
    }
    return "var(--color-text-muted)";
};

function getRuntimeOsLabel(): string {
    const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
    const platform = nav.userAgentData?.platform || navigator.platform || "";
    const ua = navigator.userAgent || "";
    const raw = `${platform} ${ua}`.toLowerCase();
    if (raw.includes("windows") || raw.includes("win32") || raw.includes("win64")) return "Windows";
    if (raw.includes("mac")) return "macOS";
    if (raw.includes("linux")) return "Linux";
    if (raw.includes("android")) return "Android";
    if (raw.includes("iphone") || raw.includes("ipad") || raw.includes("ios")) return "iOS";
    return platform || "OS";
}

// Stable per-record key so expand state survives filtering/search re-renders
// (filtered-array indices shift; the timestamp+message does not).
const recordKey = (r: LogRecord) => `${r.date} ${r.timestamp} ${r.message} ${r.occurrences ?? 1}`;

export default function LogViewer() {
    const confirmAction = useAppConfirm();
    const [records, setRecords] = useState<LogRecord[]>([]);
    const [loading, setLoading] = useState(false);
    const [level, setLevel] = useState<LevelFilter>("ERROR_WARN");
    const [source, setSource] = useState<SourceFilter>("ALL");
    const [search, setSearch] = useState("");
    const [expanded, setExpanded] = useState<Set<string>>(new Set());

    const refresh = useCallback(async () => {
        setLoading(true);
        try {
            const levels = levelFilterToBackendLevels(level);
            const data = await invoke<LogRecord[]>("get_log_records", {
                limit: 500,
                levels: levels ?? null,
            });
            setRecords(data);
            setExpanded(new Set());
        } catch {
            showError("Failed to load logs.");
        } finally {
            setLoading(false);
        }
    }, [level]);

    useEffect(() => { refresh(); }, [refresh]);

    const handleClear = useCallback(async () => {
        const accepted = await confirmAction({
            title: "Clear all WinCommander logs?",
            description: "This permanently removes every stored WinCommander log record. This cannot be undone.",
            confirmLabel: "Clear logs",
        });
        if (!accepted) return;
        try {
            await invoke("clear_log_records");
            setRecords([]);
            showSuccess("Logs cleared.");
        } catch {
            showError("Failed to clear logs.");
        }
    }, [confirmAction]);

    const filtered = useMemo(
        () => filterLogRecords(records, level, source, search),
        [records, level, source, search],
    );

    // Collapse near-simultaneous duplicate lines for the same event (e.g. a React
    // error boundary echo landing next to our own componentDidCatch line) before
    // the exact-match grouping below.
    const deduped = useMemo(() => dedupNearSimultaneous(filtered), [filtered]);

    const grouped = useMemo(() => groupLogRecords(deduped), [deduped]);
    const runtimeOsLabel = useMemo(() => getRuntimeOsLabel(), []);

    const handleCopy = useCallback(async () => {
        const text = grouped
            .map((r) => `${r.date} ${r.timestamp} [${r.level}] [${r.source}] [${r.os ?? runtimeOsLabel}] ${r.message}`)
            .join("\n");
        try {
            await navigator.clipboard.writeText(text);
            showSuccess("Plaintext logs copied to clipboard.");
        } catch {
            showError("Failed to copy logs.");
        }
    }, [grouped, runtimeOsLabel]);

    const toggleRow = useCallback((key: string) => {
        setExpanded((prev) => {
            const next = new Set(prev);
            if (next.has(key)) next.delete(key); else next.add(key);
            return next;
        });
    }, []);

    return (
        <div className="log-viewer">
            <div className="log-viewer-toolbar">
                <div className="log-viewer-filters">
                    <div className="log-level-chips">
                        {LEVEL_FILTERS.map((l) => (
                            <button
                                key={l}
                                type="button"
                                onClick={() => setLevel(l)}
                                className={`log-level-chip${level === l ? " active" : ""}`}
                                style={level === l && l !== "ALL"
                                    ? { borderColor: LEVEL_COLOR[l], color: LEVEL_COLOR[l] }
                                    : undefined}
                            >
                                {LEVEL_LABEL[l]}
                            </button>
                        ))}
                    </div>
                    <div className="log-level-chips">
                        {SOURCES.map((s) => (
                            <button
                                key={s}
                                type="button"
                                onClick={() => setSource(s)}
                                className={`log-level-chip log-source-chip${source === s ? " active" : ""}`}
                            >
                                {s}
                            </button>
                        ))}
                    </div>
                </div>
                <div className="flex gap-2">
                    <Button
                        text="Refresh"
                        icon="refresh"
                        className="compact-action-btn"
                        loading={loading}
                        onClick={refresh}
                    />
                    <Button
                        text="Copy (plaintext)"
                        icon="duplicate"
                        className="compact-action-btn"
                        disabled={grouped.length === 0}
                        onClick={handleCopy}
                    />
                    <Button
                        text="Clear"
                        icon="trash"
                        className="compact-action-btn"
                        intent="danger"
                        onClick={handleClear}
                    />
                </div>
            </div>

            <input
                type="text"
                className="log-viewer-search"
                placeholder="Search messages…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
            />

            <div className="log-viewer-list">
                {grouped.length === 0 && (
                    <div className="log-viewer-empty">
                        <Icon icon="document" size={18} />
                        <span>{loading ? "Loading…" : "No records."}</span>
                    </div>
                )}
                {grouped.map((r) => {
                    const key = recordKey(r);
                    const osLabel = r.os ?? runtimeOsLabel;
                    return (
                        <div
                            key={key}
                            className={`log-viewer-row${expanded.has(key) ? " expanded" : ""}`}
                            onClick={() => toggleRow(key)}
                            title="Click to expand"
                        >
                            <span className="log-viewer-ts">
                                {r.date} {r.timestamp}
                            </span>
                            <span
                                className="log-viewer-level"
                                style={{ color: colorForRecordLevel(r.level) }}
                            >
                                {r.level}
                            </span>
                            <span className="log-viewer-src">{r.source.toUpperCase()}</span>
                            <span className="log-viewer-os" title={`Operating system: ${osLabel}`}>
                                {osLabel}
                            </span>
                            <span className="log-viewer-msg">{r.message}</span>
                            {(r.occurrences ?? 1) > 1 && (
                                <span className="log-viewer-count" title={`First ${r.firstSeen ?? ''} · Last ${r.lastSeen ?? ''}`}>×{r.occurrences}</span>
                            )}
                        </div>
                    );
                })}
            </div>

            <div className="log-viewer-footer">
                {grouped.length > 0 && (
                    <span>{grouped.length} record{grouped.length !== 1 ? "s" : ""} — encrypted on disk</span>
                )}
            </div>
        </div>
    );
}
