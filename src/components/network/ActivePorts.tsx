import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Icon, InputGroup, HTMLSelect } from "@/components/ui/bp";
import {
    ArrowDownToLine,
    ArrowDownLeft,
    ArrowUpRight,
    Hourglass,
    CircleDashed,
    ChevronDown,
    ChevronRight,
} from "lucide-react";
import useBackend, { type NetworkPortRow, type NetworkPortsResult } from "../../hooks/useBackend";
import SectionCard from "../shared/SectionCard";
import WCSwitch from "../shared/WCSwitch";
import {
    getServiceName,
    inferDirection,
    isLoopback,
    type PortDirection,
} from "./portCatalog";
import "./ActivePorts.css";

type ProtoFilter = "all" | "tcp" | "udp";
type StateFilter = "all" | "listen" | "established" | "non-microsoft";
type DirectionFilter = "all" | "incoming" | "outgoing" | "listen";
type GroupMode = "process" | "port" | "flat";

const POLL_INTERVAL_MS = 10_000;

const MICROSOFT_PROCESS_HINTS = [
    "svchost", "system", "lsass", "services", "wininit", "smss",
    "csrss", "winlogon", "spoolsv", "fontdrvhost", "audiodg",
    "searchhost", "searchindexer", "explorer", "dllhost", "rundll32",
    "wuauclt", "msmpeng", "securityhealthservice", "trustedinstaller",
    "registry", "memory compression", "idle",
];

function looksMicrosoft(name: string | null | undefined): boolean {
    if (!name) return false;
    const lower = name.toLowerCase();
    return MICROSOFT_PROCESS_HINTS.some((h) => lower === h || lower.startsWith(`${h}.`));
}

interface EnrichedRow extends NetworkPortRow {
    direction: PortDirection;
    localService: string | null;
    remoteService: string | null;
    isLoopback: boolean;
}

function enrich(row: NetworkPortRow): EnrichedRow {
    return {
        ...row,
        direction: inferDirection(row),
        localService: getServiceName(row.localPort),
        remoteService: row.remotePort > 0 ? getServiceName(row.remotePort) : null,
        isLoopback: isLoopback(row.localAddr) || (!!row.remoteAddr && isLoopback(row.remoteAddr)),
    };
}

function matchesFilter(
    row: EnrichedRow,
    proto: ProtoFilter,
    state: StateFilter,
    direction: DirectionFilter,
    hideLoopback: boolean,
    showWindowsProcesses: boolean,
    search: string
): boolean {
    if (proto !== "all" && row.proto.toLowerCase() !== proto) return false;
    if (hideLoopback && row.isLoopback) return false;
    if (!showWindowsProcesses && looksMicrosoft(row.processName)) return false;

    if (state === "listen" && row.state !== "LISTEN") return false;
    if (state === "established" && row.state !== "ESTABLISHED") return false;
    if (state === "non-microsoft" && looksMicrosoft(row.processName)) return false;

    if (direction !== "all") {
        if (direction === "listen" && row.direction !== "listen") return false;
        if (direction === "incoming" && row.direction !== "incoming") return false;
        if (direction === "outgoing" && row.direction !== "outgoing") return false;
    }

    if (search) {
        const q = search.toLowerCase();
        const haystack = [
            row.processName ?? "",
            row.localAddr,
            row.remoteAddr,
            String(row.localPort),
            String(row.remotePort),
            String(row.pid),
            row.localService ?? "",
            row.remoteService ?? "",
        ].join(" ").toLowerCase();
        if (!haystack.includes(q)) return false;
    }
    return true;
}

const DIRECTION_META: Record<PortDirection, { label: string; color: string; Icon: typeof ArrowDownToLine }> = {
    listen:   { label: "LISTEN",   color: "var(--color-accent)",       Icon: ArrowDownToLine },
    incoming: { label: "IN",       color: "var(--color-warning)",      Icon: ArrowDownLeft },
    outgoing: { label: "OUT",      color: "var(--color-text-muted)",   Icon: ArrowUpRight },
    closing:  { label: "CLOSING",  color: "var(--color-text-muted)",   Icon: Hourglass },
    pending:  { label: "PENDING",  color: "var(--color-text-muted)",   Icon: CircleDashed },
};

function DirectionBadge({ direction }: { direction: PortDirection }) {
    const meta = DIRECTION_META[direction];
    const Glyph = meta.Icon;
    return (
        <span
            title={meta.label}
            style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 4,
                color: meta.color,
            }}
        >
            <Glyph size={12} />
            <span style={{ fontSize: 9, fontWeight: 700, letterSpacing: 0.5 }}>{meta.label}</span>
        </span>
    );
}

function endpointDisplay(addr: string, port: number, service: string | null): string {
    const a = addr === "0.0.0.0" || addr === "::" ? "*" : addr;
    const portLabel = service ? `${port} ${service}` : String(port);
    return `${a}:${portLabel}`;
}

interface PortRowViewProps { row: EnrichedRow }
function PortRowView({ row }: PortRowViewProps) {
    return (
        <tr style={{ borderTop: "1px solid var(--color-border)" }}>
            <td style={{ padding: "4px 8px" }}>
                <DirectionBadge direction={row.direction} />
            </td>
            <td
                style={{ padding: "4px 8px", color: row.proto === "TCP" ? "var(--color-accent)" : "var(--color-warning)" }}
            >
                {row.proto}
            </td>
            <td
                style={{ padding: "4px 8px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                title={`${row.localAddr}:${row.localPort}${row.localService ? " (" + row.localService + ")" : ""}`}
            >
                {endpointDisplay(row.localAddr, row.localPort, row.localService)}
            </td>
            <td
                style={{ padding: "4px 8px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                title={row.remoteAddr ? `${row.remoteAddr}:${row.remotePort}${row.remoteService ? " (" + row.remoteService + ")" : ""}` : ""}
            >
                {row.remoteAddr ? endpointDisplay(row.remoteAddr, row.remotePort, row.remoteService) : "—"}
            </td>
            <td style={{ padding: "4px 8px", opacity: 0.85 }}>{row.state}</td>
            <td style={{ padding: "4px 8px", opacity: 0.7 }}>{row.pid || "—"}</td>
        </tr>
    );
}

interface Group {
    key: string;
    title: string;
    subtitle?: string;
    rows: EnrichedRow[];
    isMicrosoft: boolean;
}

function buildGroups(rows: EnrichedRow[], mode: GroupMode): Group[] {
    if (mode === "flat") {
        return [{ key: "__all__", title: "All connections", rows, isMicrosoft: false }];
    }

    const map = new Map<string, Group>();

    for (const row of rows) {
        let key: string;
        let title: string;
        let subtitle: string | undefined;
        let isMs = false;

        if (mode === "process") {
            const name = row.processName ?? "(unknown)";
            key = `${name}:${row.pid}`;
            title = name;
            subtitle = row.pid ? `PID ${row.pid}` : undefined;
            isMs = looksMicrosoft(row.processName);
        } else {
            // mode === "port" — group by local port (after enrichment)
            key = `${row.proto}:${row.localPort}`;
            const svc = row.localService ? ` ${row.localService}` : "";
            title = `${row.proto} ${row.localPort}${svc}`;
        }

        let g = map.get(key);
        if (!g) {
            g = { key, title, subtitle, rows: [], isMicrosoft: isMs };
            map.set(key, g);
        }
        g.rows.push(row);
    }

    return Array.from(map.values()).sort((a, b) => {
        // Most-active groups first; non-Microsoft above Microsoft so users
        // notice unfamiliar processes before scrolling past the OS noise.
        if (a.isMicrosoft !== b.isMicrosoft) return a.isMicrosoft ? 1 : -1;
        if (a.rows.length !== b.rows.length) return b.rows.length - a.rows.length;
        return a.title.localeCompare(b.title);
    });
}

interface GroupBlockProps {
    group: Group;
    expanded: boolean;
    onToggle: () => void;
    showHeader: boolean;
}
function GroupBlock({ group, expanded, onToggle, showHeader }: GroupBlockProps) {
    return (
        <>
            {showHeader && (
                <tr
                    onClick={onToggle}
                    style={{
                        cursor: "pointer",
                        background: "var(--color-bg-tertiary)",
                        borderTop: "1px solid var(--color-border)",
                    }}
                >
                    <td colSpan={6} style={{ padding: "6px 8px" }}>
                        <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
                            {expanded
                                ? <ChevronDown size={11} />
                                : <ChevronRight size={11} />}
                            <span
                                style={{
                                    fontWeight: 700,
                                    color: group.isMicrosoft
                                        ? "var(--color-text-muted)"
                                        : "var(--color-text-primary)",
                                }}
                            >
                                {group.title}
                            </span>
                            {group.subtitle && (
                                <span style={{ fontSize: 9, opacity: 0.6 }}>
                                    {group.subtitle}
                                </span>
                            )}
                            <span
                                className="hardware-process-count"
                                style={{ marginLeft: "auto" }}
                            >
                                {group.rows.length}
                            </span>
                        </span>
                    </td>
                </tr>
            )}
            {expanded && group.rows.map((r, idx) => (
                <PortRowView
                    key={`${r.proto}-${r.pid}-${r.localAddr}-${r.localPort}-${r.remoteAddr}-${r.remotePort}-${idx}`}
                    row={r}
                />
            ))}
        </>
    );
}

export default function ActivePorts() {
    const { getNetworkPorts } = useBackend();
    const [data, setData] = useState<NetworkPortsResult | null>(null);
    const [loading, setLoading] = useState(false);
    const [lastError, setLastError] = useState<string | null>(null);
    const [paused, setPaused] = useState(false);

    const [proto, setProto] = useState<ProtoFilter>("all");
    const [state, setState] = useState<StateFilter>("all");
    const [direction, setDirection] = useState<DirectionFilter>("all");
    const [hideLoopback, setHideLoopback] = useState(false);
    const [showWindowsProcesses, setShowWindowsProcesses] = useState(false);
    const [search, setSearch] = useState("");
    const [groupMode, setGroupMode] = useState<GroupMode>("process");
    // Default: every group collapsed. Membership in this set means the user
    // has explicitly expanded that group. An active search overrides and
    // auto-expands all groups so matched rows aren't hidden.
    const [expanded, setExpanded] = useState<Set<string>>(new Set());

    const inFlight = useRef(false);

    const refresh = useCallback(async (silent: boolean) => {
        if (inFlight.current) return;
        inFlight.current = true;
        if (!silent) setLoading(true);
        try {
            const res = await getNetworkPorts(1500);
            if (res.success && res.data) {
                setData(res.data);
                setLastError(null);
            } else if (res.error) {
                setLastError(res.error);
            }
        } finally {
            inFlight.current = false;
            if (!silent) setLoading(false);
        }
    }, [getNetworkPorts]);

    useEffect(() => {
        refresh(false);
        if (paused) return;
        const id = setInterval(() => refresh(true), POLL_INTERVAL_MS);
        return () => clearInterval(id);
    }, [refresh, paused]);

    useEffect(() => {
        const onVis = () => setPaused(document.hidden);
        document.addEventListener("visibilitychange", onVis);
        return () => document.removeEventListener("visibilitychange", onVis);
    }, []);

    const enrichedRows: EnrichedRow[] = useMemo(() => (data?.rows ?? []).map(enrich), [data]);

    const filteredRows = useMemo(
        () => enrichedRows.filter((r) => matchesFilter(r, proto, state, direction, hideLoopback, showWindowsProcesses, search.trim())),
        [enrichedRows, proto, state, direction, hideLoopback, showWindowsProcesses, search]
    );

    const groups = useMemo(() => buildGroups(filteredRows, groupMode), [filteredRows, groupMode]);

    const totalsBadge = data
        ? `${data.totals?.tcp ?? 0} TCP · ${data.totals?.udp ?? 0} UDP · showing ${filteredRows.length}/${data.totals?.shown ?? data.rows?.length ?? 0}${data.truncated ? " (capped)" : ""}`
        : loading ? "Scanning…" : "—";

    const toggleGroup = (key: string) => {
        setExpanded((prev) => {
            const next = new Set(prev);
            if (next.has(key)) next.delete(key); else next.add(key);
            return next;
        });
    };

    const searchActive = search.trim().length > 0;

    return (
        <SectionCard
            title="Active Ports"
            icon="globe-network"
            headerRight={
                <div className="flex items-center gap-3">
                    <span className="font-mono text-[10px] tracking-wider text-[var(--color-text-muted)]">
                        {totalsBadge}
                    </span>
                    <button
                        type="button"
                        className="refresh-btn"
                        onClick={() => refresh(false)}
                        disabled={loading}
                        title="Refresh now"
                    >
                        <Icon icon="refresh" size={14} className={loading ? "spinning" : ""} />
                    </button>
                </div>
            }
        >
            <div className="flex flex-col gap-3">
                {/* Filter controls — sit directly under the section heading,
                    bold "Filter:" label leads the row. */}
                <div className="active-ports-filter-bar">
                    <span className="active-ports-filter-bar__label">Filter:</span>
                    <InputGroup
                        placeholder="Filter by process, IP, port, service, PID…"
                        value={search}
                        onChange={(e) => setSearch(e.target.value)}
                        leftIcon="filter"
                        small
                        className="active-ports-search"
                    />
                    <HTMLSelect
                        value={proto}
                        onChange={(e) => setProto(e.currentTarget.value as ProtoFilter)}
                        options={[
                            { value: "all", label: "All protocols" },
                            { value: "tcp", label: "TCP only" },
                            { value: "udp", label: "UDP only" },
                        ]}
                        minimal
                    />
                    <HTMLSelect
                        value={state}
                        onChange={(e) => setState(e.currentTarget.value as StateFilter)}
                        options={[
                            { value: "all", label: "Any state" },
                            { value: "listen", label: "LISTEN only" },
                            { value: "established", label: "ESTABLISHED only" },
                            { value: "non-microsoft", label: "Non-Microsoft only" },
                        ]}
                        minimal
                    />
                    <HTMLSelect
                        value={direction}
                        onChange={(e) => setDirection(e.currentTarget.value as DirectionFilter)}
                        options={[
                            { value: "all", label: "Any direction" },
                            { value: "listen", label: "Listening" },
                            { value: "incoming", label: "Incoming" },
                            { value: "outgoing", label: "Outgoing" },
                        ]}
                        minimal
                    />
                    <HTMLSelect
                        value={groupMode}
                        onChange={(e) => setGroupMode(e.currentTarget.value as GroupMode)}
                        options={[
                            { value: "process", label: "Group by process" },
                            { value: "port", label: "Group by port" },
                            { value: "flat", label: "Flat list" },
                        ]}
                        minimal
                    />
                </div>

                {/* Consistent toggle row — one control style (WCSwitch) for all
                    three on/off filter behaviors, each self-explanatory about
                    what it applies to.
                    Each row is itself the accessible switch (role="switch" +
                    aria-checked + keyboard support), matching the ToggleTile
                    pattern: the visible text IS the control's label, and the
                    nested WCSwitch is a purely visual echo whose own onClick
                    stops propagation so a click never double-toggles. */}
                <div className="active-ports-toggle-bar">
                    <div
                        className="active-ports-toggle"
                        title="Show processes owned by Windows/Microsoft (svchost, services, explorer, etc.)"
                        role="switch"
                        aria-checked={showWindowsProcesses}
                        tabIndex={0}
                        onClick={() => setShowWindowsProcesses((v) => !v)}
                        onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                                e.preventDefault();
                                setShowWindowsProcesses((v) => !v);
                            }
                        }}
                    >
                        <WCSwitch checked={showWindowsProcesses} onChange={setShowWindowsProcesses} size="sm" />
                        <span>Show Windows System Processes</span>
                    </div>
                    <div
                        className="active-ports-toggle"
                        title="Hide connections to/from 127.0.0.1 or ::1 (this PC talking to itself)"
                        role="switch"
                        aria-checked={hideLoopback}
                        tabIndex={0}
                        onClick={() => setHideLoopback((v) => !v)}
                        onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                                e.preventDefault();
                                setHideLoopback((v) => !v);
                            }
                        }}
                    >
                        <WCSwitch checked={hideLoopback} onChange={setHideLoopback} size="sm" />
                        <span>Hide Loopback Connections</span>
                    </div>
                    <div
                        className="active-ports-toggle"
                        title="Pause the 10-second automatic refresh of this list"
                        role="switch"
                        aria-checked={paused}
                        tabIndex={0}
                        onClick={() => setPaused((v) => !v)}
                        onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                                e.preventDefault();
                                setPaused((v) => !v);
                            }
                        }}
                    >
                        <WCSwitch checked={paused} onChange={setPaused} size="sm" />
                        <span>Pause Port Monitoring</span>
                    </div>
                </div>

                {lastError && (
                    <div
                        className="font-mono text-[11px]"
                        style={{ color: "var(--color-danger)" }}
                    >
                        {lastError}
                    </div>
                )}

                {/* Table */}
                <div className="custom-scrollbar" style={{ maxHeight: 420, overflowY: "auto" }}>
                    <table
                        className="active-ports-table font-mono"
                        style={{ width: "100%", borderCollapse: "collapse", tableLayout: "fixed" }}
                    >
                        <colgroup>
                            <col style={{ width: 88 }} />
                            <col style={{ width: 50 }} />
                            <col />
                            <col />
                            <col style={{ width: 110 }} />
                            <col style={{ width: 60 }} />
                        </colgroup>
                        <thead
                            style={{
                                position: "sticky",
                                top: 0,
                                background: "var(--color-bg-elevated)",
                                zIndex: 1,
                            }}
                        >
                            <tr style={{ textAlign: "left", color: "var(--color-text-muted)" }}>
                                <th style={{ padding: "6px 8px" }}>DIR</th>
                                <th style={{ padding: "6px 8px" }}>PROTO</th>
                                <th style={{ padding: "6px 8px" }}>LOCAL</th>
                                <th style={{ padding: "6px 8px" }}>REMOTE</th>
                                <th style={{ padding: "6px 8px" }}>STATE</th>
                                <th style={{ padding: "6px 8px" }}>PID</th>
                            </tr>
                        </thead>
                        <tbody>
                            {filteredRows.length === 0 ? (
                                <tr>
                                    <td
                                        colSpan={6}
                                        style={{
                                            padding: 24,
                                            textAlign: "center",
                                            color: "var(--color-text-muted)",
                                        }}
                                    >
                                        {loading ? "Scanning sockets…" : "No connections match the current filters."}
                                    </td>
                                </tr>
                            ) : (
                                groups.map((g) => (
                                    <GroupBlock
                                        key={g.key}
                                        group={g}
                                        // Flat mode renders every row anyway (no header). Search
                                        // overrides the per-group toggle so matched rows surface.
                                        expanded={groupMode === "flat" || searchActive || expanded.has(g.key)}
                                        onToggle={() => toggleGroup(g.key)}
                                        showHeader={groupMode !== "flat"}
                                    />
                                ))
                            )}
                        </tbody>
                    </table>
                </div>
            </div>
        </SectionCard>
    );
}
