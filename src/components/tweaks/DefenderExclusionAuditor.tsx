import { useCallback, useEffect, useState } from "react";
import { Icon, Button } from "@/components/ui/bp";
import useBackend, {
    type DefenderExclusionRow,
    type DefenderExclusionSeverity,
    type DefenderExclusionsResult,
} from "../../hooks/useBackend";
import SectionCard from "../shared/SectionCard";

const KIND_LABEL: Record<DefenderExclusionRow["kind"], string> = {
    path: "PATH",
    process: "PROCESS",
    extension: "EXT",
    ip: "IP",
};

function severityColor(sev: DefenderExclusionSeverity): string {
    switch (sev) {
        case "critical": return "var(--color-danger)";
        case "high":     return "var(--color-warning)";
        case "info":     return "var(--color-text-muted)";
    }
}

function severityLabel(sev: DefenderExclusionSeverity): string {
    return sev.toUpperCase();
}

export default function DefenderExclusionAuditor() {
    const { getDefenderExclusions } = useBackend();
    const [data, setData] = useState<DefenderExclusionsResult | null>(null);
    const [loading, setLoading] = useState(true);

    const refresh = useCallback(async () => {
        setLoading(true);
        try {
            const res = await getDefenderExclusions();
            if (res.success && res.data) setData(res.data);
            else if (res.error) setData({ status: "unavailable", error: res.error });
        } finally {
            setLoading(false);
        }
    }, [getDefenderExclusions]);

    useEffect(() => { refresh(); }, [refresh]);

    if (loading && !data) {
        return (
            <SectionCard title="Defender Exclusion Auditor" icon="shield" className="defender-exclusion-auditor-card">
                <div className="font-mono text-[11px] opacity-70 py-3">Reading Defender preferences…</div>
            </SectionCard>
        );
    }

    // Defender intentionally disabled — render as a calm, informational state.
    // This is a valid end-state of the app's own sovereignty/debloat tweaks,
    // not an error condition.
    if (data?.status === "disabled") {
        return (
            <SectionCard title="Defender Exclusion Auditor" icon="shield" className="defender-exclusion-auditor-card">
                <div className="font-mono text-[11px] py-2 flex items-center gap-2" style={{ color: "var(--color-text-muted)" }}>
                    <Icon icon="info-sign" size={12} />
                    <span>{data.message}</span>
                </div>
            </SectionCard>
        );
    }

    if (!data || data.status === "unavailable") {
        return (
            <SectionCard title="Defender Exclusion Auditor" icon="shield" className="defender-exclusion-auditor-card">
                <div className="font-mono text-[11px]" style={{ color: "var(--color-danger)" }}>
                    {data?.status === "unavailable" ? data.error : "Unavailable"}
                </div>
            </SectionCard>
        );
    }

    // KT: PS sometimes returns the row array as a non-iterable scalar when the
    // underlying list has 0 or 1 element (the JSON serializer collapses single
    // hashtables into objects, and missing keys come back as undefined).
    // Coerce defensively.
    const total = data.total ?? 0;
    const bySeverity = data.bySeverity ?? { critical: 0, high: 0, info: 0 };
    const rawRows = data.rows;
    const rows = Array.isArray(rawRows) ? rawRows : (rawRows ? [rawRows] : []);
    const sortedRows = [...rows].sort((a, b) => {
        const order = { critical: 0, high: 1, info: 2 };
        return order[a.severity] - order[b.severity];
    });

    return (
        <SectionCard
            title="Defender Exclusion Auditor"
            icon="shield"
            className="defender-exclusion-auditor-card"
            headerRight={
                <div className="flex items-center gap-3">
                    <span className="font-mono text-[10px] tracking-wider">
                        <span style={{ color: severityColor("critical") }}>{bySeverity.critical} CRIT</span>
                        <span className="opacity-50"> · </span>
                        <span style={{ color: severityColor("high") }}>{bySeverity.high} HIGH</span>
                        <span className="opacity-50"> · </span>
                        <span style={{ color: severityColor("info") }}>{bySeverity.info} OK</span>
                    </span>
                    <Button small minimal icon="refresh" onClick={refresh} loading={loading} />
                </div>
            }
        >
            {total === 0 ? (
                <div className="font-mono text-[11px] py-3" style={{ color: "var(--color-success, #10b981)" }}>
                    ✓ No Defender exclusions configured. This is the safe default.
                </div>
            ) : (
                <>
                    <div className="font-mono text-[10px] opacity-70 mb-2">
                        Read-only audit. Malware commonly persists by adding Defender exclusions for
                        drive roots, user-writable directories, or scripting hosts (powershell, mshta, wscript).
                        Review these manually in Windows Security if any look unfamiliar.
                    </div>

                    <div className="defender-exclusion-auditor-table-wrap custom-scrollbar">
                        <table
                            className="font-mono text-[11px]"
                            style={{ width: "100%", borderCollapse: "collapse", tableLayout: "fixed" }}
                        >
                            <thead
                                style={{
                                    position: "sticky",
                                    top: 0,
                                    background: "var(--color-bg-elevated)",
                                    zIndex: 1,
                                }}
                            >
                                <tr style={{ textAlign: "left", color: "var(--color-text-muted)" }}>
                                    <th style={{ width: 70, padding: "6px 8px" }}>SEV</th>
                                    <th style={{ width: 90, padding: "6px 8px" }}>KIND</th>
                                    <th style={{ padding: "6px 8px" }}>VALUE</th>
                                </tr>
                            </thead>
                            <tbody>
                                {sortedRows.map((r, idx) => (
                                    <tr
                                        key={`${r.kind}-${r.value}-${idx}`}
                                        style={{ borderTop: "1px solid var(--color-border)" }}
                                    >
                                        <td style={{ padding: "4px 8px" }}>
                                            <span
                                                style={{
                                                    fontWeight: 700,
                                                    color: severityColor(r.severity),
                                                }}
                                            >
                                                {r.severity === "critical" && (
                                                    <Icon
                                                        icon="warning-sign"
                                                        size={11}
                                                        color={severityColor(r.severity)}
                                                        style={{ marginRight: 4 }}
                                                    />
                                                )}
                                                {severityLabel(r.severity)}
                                            </span>
                                        </td>
                                        <td style={{ padding: "4px 8px", opacity: 0.7 }}>
                                            {KIND_LABEL[r.kind]}
                                        </td>
                                        <td
                                            style={{
                                                padding: "4px 8px",
                                                overflow: "hidden",
                                                textOverflow: "ellipsis",
                                                whiteSpace: "nowrap",
                                            }}
                                            title={r.value}
                                        >
                                            {r.value}
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>
                </>
            )}
        </SectionCard>
    );
}
